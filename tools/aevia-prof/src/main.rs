//! `aevia-prof`: a sampling profiler for Windows that needs no administrator
//! rights.
//!
//! ETW-based profilers (WPR, samply) need elevation. This tool instead uses
//! the debugging API that any user has over their own processes: it starts
//! the command, and every millisecond it suspends each thread of the child,
//! reads its register context, walks its stack with `dbghelp` (which reads the
//! x64 unwind tables of the loaded modules), and resumes it. At the end it
//! prints the functions with the most self samples and the most inclusive
//! samples. Build the profiled binary with debug info (the `profiling` cargo
//! profile does) so symbols resolve.
//!
//! ```console
//! cargo build --profile profiling -p aevia-cli
//! cargo run -p aevia-prof --release -- target/profiling/aevia.exe run prog.aev
//! ```
//!
//! Environment: `AEVIA_PROF_TOP` (rows printed, default 45),
//! `AEVIA_PROF_INTERVAL_US` (sampling interval, default 1000), and
//! `AEVIA_PROF_FOLDED` (optional new file for `FlameGraph` folded stacks).

#![deny(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::nursery,
    clippy::pedantic,
    clippy::single_call_fn,
    clippy::unwrap_used
)]

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
mod report;

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!("aevia-prof requires x86-64 Windows");
    std::process::exit(2);
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    std::process::exit(windows::run());
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[allow(unsafe_code, reason = "Win32 handles and DbgHelp require FFI calls")]
mod windows {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::io::Write as _;
    use std::time::Duration;
    use std::time::Instant;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Diagnostics::Debug::AddrModeFlat;
    use windows_sys::Win32::System::Diagnostics::Debug::CONTEXT;
    use windows_sys::Win32::System::Diagnostics::Debug::GetThreadContext;
    use windows_sys::Win32::System::Diagnostics::Debug::IMAGEHLP_MODULE64;
    use windows_sys::Win32::System::Diagnostics::Debug::STACKFRAME64;
    use windows_sys::Win32::System::Diagnostics::Debug::SYMBOL_INFO;
    use windows_sys::Win32::System::Diagnostics::Debug::SYMOPT_UNDNAME;
    use windows_sys::Win32::System::Diagnostics::Debug::StackWalk64;
    use windows_sys::Win32::System::Diagnostics::Debug::SymCleanup;
    use windows_sys::Win32::System::Diagnostics::Debug::SymFromAddr;
    use windows_sys::Win32::System::Diagnostics::Debug::SymFunctionTableAccess64;
    use windows_sys::Win32::System::Diagnostics::Debug::SymGetModuleBase64;
    use windows_sys::Win32::System::Diagnostics::Debug::SymGetModuleInfo64;
    use windows_sys::Win32::System::Diagnostics::Debug::SymInitialize;
    use windows_sys::Win32::System::Diagnostics::Debug::SymLoadModuleEx;
    use windows_sys::Win32::System::Diagnostics::Debug::SymSetOptions;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::CreateToolhelp32Snapshot;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::MODULEENTRY32;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::Module32First;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::TH32CS_SNAPMODULE;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::TH32CS_SNAPTHREAD;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::THREADENTRY32;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::Thread32First;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::Thread32Next;
    use windows_sys::Win32::System::Threading::GetProcessIdOfThread;
    use windows_sys::Win32::System::Threading::OpenProcess;
    use windows_sys::Win32::System::Threading::OpenThread;
    use windows_sys::Win32::System::Threading::PROCESS_QUERY_INFORMATION;
    use windows_sys::Win32::System::Threading::PROCESS_VM_READ;
    use windows_sys::Win32::System::Threading::ResumeThread;
    use windows_sys::Win32::System::Threading::SuspendThread;
    use windows_sys::Win32::System::Threading::THREAD_GET_CONTEXT;
    use windows_sys::Win32::System::Threading::THREAD_QUERY_INFORMATION;
    use windows_sys::Win32::System::Threading::THREAD_SUSPEND_RESUME;

    use crate::report::Profile;

    #[repr(C, align(16))]
    struct AlignedContext(CONTEXT);

    const MAX_FRAMES: usize = 192;
    const IMAGE_FILE_MACHINE_AMD64: u32 = 0x8664;
    const CONTEXT_AMD64: u32 = 0x0010_0000;
    const CONTEXT_CONTROL_AMD64: u32 = CONTEXT_AMD64 | 0x1;
    const CONTEXT_INTEGER_AMD64: u32 = CONTEXT_AMD64 | 0x2;
    const SYMBOL_NAME_LEN: usize = 1024;
    fn record_size<T>() -> u32 {
        u32::try_from(std::mem::size_of::<T>()).unwrap_or_default()
    }

    #[repr(C)]
    struct SymbolBuffer {
        header: SYMBOL_INFO,
        name_space: [u8; SYMBOL_NAME_LEN],
    }

    struct TimerPeriod;

    impl Drop for TimerPeriod {
        fn drop(&mut self) {
            // SAFETY: This guard is created only after timeBeginPeriod(1) succeeds.
            unsafe { windows_sys::Win32::Media::timeEndPeriod(1) };
        }
    }

    #[allow(
        clippy::single_call_fn,
        reason = "each unique address is resolved through DbgHelp"
    )]
    fn symbol_name(
        process: HANDLE,
        address: u64,
        debug: bool,
    ) -> String {
        // SAFETY: The Windows C record is plain integer/array data and
        // zero is valid. repr(C) gives the header the required alignment.
        let mut buffer: SymbolBuffer = unsafe { std::mem::zeroed() };
        buffer.header.SizeOfStruct = record_size::<SYMBOL_INFO>();
        buffer.header.MaxNameLen = u32::try_from(SYMBOL_NAME_LEN).unwrap_or_default();
        let symbol = (&raw mut buffer).cast::<SYMBOL_INFO>();
        // SAFETY: DbgHelp receives a live process handle and storage for
        // the header and MaxNameLen bytes. The name slice stays in that storage.
        unsafe {
            let mut displacement = 0u64;
            if SymFromAddr(process, address, &raw mut displacement, symbol) != 0 {
                let len = (*symbol).NameLen as usize;
                let ptr = std::ptr::addr_of!((*symbol).Name).cast::<u8>();
                let bytes = std::slice::from_raw_parts(ptr, len.min(SYMBOL_NAME_LEN));
                simplify(&String::from_utf8_lossy(bytes))
            } else {
                if debug {
                    eprintln!(
                        "SymFromAddr 0x{address:x} failed: {}",
                        windows_sys::Win32::Foundation::GetLastError()
                    );
                }
                format!("0x{address:x}")
            }
        }
    }

    /// Collapses generic arguments so monomorphizations can share a row.
    #[allow(
        clippy::single_call_fn,
        reason = "symbol normalization is separate from Win32 lookup"
    )]
    fn simplify(name: &str) -> String {
        let mut out = String::with_capacity(name.len());
        let mut depth = 0usize;
        for ch in name.chars() {
            match ch {
                | '<' => {
                    if depth == 0 {
                        out.push_str("<…>");
                    }
                    depth = depth.saturating_add(1);
                },
                | '>' => depth = depth.saturating_sub(1),
                | _ if depth == 0 => out.push(ch),
                | _ => {},
            }
        }
        out
    }

    fn main_module(pid: u32) -> (u64, u64) {
        // SAFETY: The initialized entry is sized for ToolHelp, and the
        // snapshot handle is checked and closed before returning.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
            if snapshot.is_null() || snapshot as isize == -1 {
                return (0, 0);
            }
            let mut entry: MODULEENTRY32 = std::mem::zeroed();
            entry.dwSize = record_size::<MODULEENTRY32>();
            let module = if Module32First(snapshot, &raw mut entry) != 0 {
                (entry.modBaseAddr as u64, u64::from(entry.modBaseSize))
            } else {
                (0, 0)
            };
            CloseHandle(snapshot);
            module
        }
    }

    fn threads_of(pid: u32) -> Vec<u32> {
        let mut out = Vec::new();
        // SAFETY: The initialized entry and live snapshot are passed to
        // ToolHelp until iteration is finished; the handle is then closed.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot.is_null() || snapshot as isize == -1 {
                return out;
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = record_size::<THREADENTRY32>();
            if Thread32First(snapshot, &raw mut entry) != 0 {
                loop {
                    if entry.th32OwnerProcessID == pid {
                        out.push(entry.th32ThreadID);
                    }
                    if Thread32Next(snapshot, &raw mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
        }
        out
    }

    #[allow(
        clippy::single_call_fn,
        reason = "isolates the suspended-thread lifetime and safety contract"
    )]
    fn sample_thread(
        process: HANDLE,
        thread: HANDLE,
    ) -> Option<Vec<u64>> {
        // SAFETY: Both handles remain open for the call. The context has x64
        // alignment, and the caller supplies a thread from the sampled process.
        unsafe {
            if SuspendThread(thread) == u32::MAX {
                return None;
            }
            let mut context: AlignedContext = std::mem::zeroed();
            context.0.ContextFlags = CONTEXT_CONTROL_AMD64 | CONTEXT_INTEGER_AMD64;
            let mut frames = Vec::new();
            if GetThreadContext(thread, &raw mut context.0) != 0 {
                let mut frame: STACKFRAME64 = std::mem::zeroed();
                frame.AddrPC.Offset = context.0.Rip;
                frame.AddrPC.Mode = AddrModeFlat;
                frame.AddrFrame.Offset = context.0.Rbp;
                frame.AddrFrame.Mode = AddrModeFlat;
                frame.AddrStack.Offset = context.0.Rsp;
                frame.AddrStack.Mode = AddrModeFlat;
                while frames.len() < MAX_FRAMES {
                    let ok = StackWalk64(
                        IMAGE_FILE_MACHINE_AMD64,
                        process,
                        thread,
                        &raw mut frame,
                        (&raw mut context.0).cast::<c_void>(),
                        None,
                        Some(SymFunctionTableAccess64),
                        Some(SymGetModuleBase64),
                        None,
                    );
                    if ok == 0 || frame.AddrPC.Offset == 0 {
                        break;
                    }
                    frames.push(frame.AddrPC.Offset);
                }
            }
            ResumeThread(thread);
            (!frames.is_empty()).then_some(frames)
        }
    }

    #[allow(
        clippy::single_call_fn,
        clippy::too_many_lines,
        reason = "platform entry point owns the child, Windows handles and sampling lifetime"
    )]
    pub fn run() -> i32 {
        let mut args = std::env::args().skip(1);
        let Some(program) = args.next() else {
            eprintln!("usage: aevia-prof <program> [args...]");
            return 2;
        };
        let top: usize = std::env::var("AEVIA_PROF_TOP")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(45);
        let interval = Duration::from_micros(
            std::env::var("AEVIA_PROF_INTERVAL_US")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1000),
        );
        // SAFETY: The call accepts a period in milliseconds and no pointers.
        let _timer = if unsafe { windows_sys::Win32::Media::timeBeginPeriod(1) } == 0 {
            Some(TimerPeriod)
        } else {
            None
        };
        let mut child = match std::process::Command::new(&program).args(args).spawn() {
            | Ok(child) => child,
            | Err(error) => {
                eprintln!("aevia-prof: cannot start {program}: {error}");
                return 2;
            },
        };
        let pid = child.id();
        // SAFETY: pid names the child we just started; the returned handle is checked.
        let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if process.is_null() {
            eprintln!("aevia-prof: cannot open the child process");
            let _ = child.wait();
            return 2;
        }
        std::thread::sleep(Duration::from_millis(30));
        let debug = std::env::var_os("AEVIA_PROF_DEBUG").is_some();
        // SAFETY: DbgHelp calls are serialized on this thread. The process
        // handle and C strings remain live through the calls.
        unsafe {
            SymSetOptions(SYMOPT_UNDNAME);
            // Load the main image explicitly at its actual (ASLR) base once
            // the loader has mapped it; system modules are not symbolized.
            let mut base = 0;
            for _ in 0..200 {
                base = main_module(pid).0;
                if base != 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let image_path = std::fs::canonicalize(&program).map_or_else(
                |_| program.replace('/', "\\"),
                |path| {
                    let text = path.display().to_string();
                    text.strip_prefix(r"\\?\")
                        .map_or_else(|| text.clone(), str::to_owned)
                },
            );
            let directory = std::path::Path::new(&image_path)
                .parent()
                .map_or_default(|dir| dir.display().to_string());
            let search = std::ffi::CString::new(directory).unwrap_or_default();
            let ok = SymInitialize(process, search.as_ptr().cast(), 0);
            let image = std::ffi::CString::new(image_path.clone()).unwrap_or_default();
            if debug {
                eprintln!("image {image_path}");
            }
            let loaded = SymLoadModuleEx(
                process,
                std::ptr::null_mut(),
                image.as_ptr().cast(),
                std::ptr::null(),
                base,
                0,
                std::ptr::null(),
                0,
            );
            if debug {
                let mut info: IMAGEHLP_MODULE64 = std::mem::zeroed();
                info.SizeOfStruct = record_size::<IMAGEHLP_MODULE64>();
                let got = SymGetModuleInfo64(process, base, &raw mut info);
                let pdb = String::from_utf8_lossy(
                    &info
                        .LoadedPdbName
                        .iter()
                        .map(|c| u8::from_ne_bytes(c.to_ne_bytes()))
                        .take_while(|&c| c != 0)
                        .collect::<Vec<u8>>(),
                )
                .into_owned();
                eprintln!(
                    "SymInitialize = {ok}; image base 0x{base:x}; loaded 0x{loaded:x}; info {got} symtype {} pdb {pdb}",
                    info.SymType
                );
            }
        }
        let mut handles: HashMap<u32, HANDLE> = HashMap::new();
        let mut stacks: HashMap<Vec<u64>, u64> = HashMap::new();
        let (image_base, image_size) = main_module(pid);
        let in_image = |pc: u64| {
            pc.checked_sub(image_base)
                .is_some_and(|offset| offset < image_size)
        };
        let started = Instant::now();
        // Enumerating threads walks every thread in the system, so the list
        // is refreshed only every 100 ms.
        let mut threads = threads_of(pid);
        let mut listed = Instant::now();
        let status = loop {
            if let Ok(Some(status)) = child.try_wait() {
                break status.code().unwrap_or(1);
            }
            if listed.elapsed() > Duration::from_millis(100) {
                threads = threads_of(pid);
                listed = Instant::now();
            }
            for &tid in &threads {
                let handle = match handles.entry(tid) {
                    | std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                    | std::collections::hash_map::Entry::Vacant(entry) => {
                        // SAFETY: OpenThread takes a numeric ID. The handle is checked and
                        // retained only when it still belongs to the child process.
                        let handle = unsafe {
                            OpenThread(
                                THREAD_SUSPEND_RESUME
                                    | THREAD_GET_CONTEXT
                                    | THREAD_QUERY_INFORMATION,
                                0,
                                tid,
                            )
                        };
                        if handle.is_null() {
                            continue;
                        }
                        // SAFETY: handle is live until this block closes or records it.
                        if unsafe { GetProcessIdOfThread(handle) } != pid {
                            // SAFETY: No other code has received this handle.
                            unsafe { CloseHandle(handle) };
                            continue;
                        }
                        *entry.insert(handle)
                    },
                };
                if let Some(mut frames) = sample_thread(process, handle) {
                    // A thread parked in the system (the CLI's main thread
                    // joining the interpreter thread) has no frame in the
                    // image; frames past the image's outermost one are the
                    // unsymbolized thread start.
                    let Some(last) = frames.iter().rposition(|&pc| in_image(pc)) else {
                        continue;
                    };
                    frames.truncate(last.saturating_add(1));
                    // System frames on top (heap calls) are kept as one leaf.
                    let samples = stacks.entry(frames).or_default();
                    *samples = samples.saturating_add(1);
                }
            }
            std::thread::sleep(interval);
        };
        let wall = started.elapsed();
        for handle in handles.values() {
            if !handle.is_null() {
                // SAFETY: Each retained thread handle was opened once and is no longer sampled.
                unsafe {
                    CloseHandle(*handle);
                }
            }
        }
        let folded = std::env::var_os("AEVIA_PROF_FOLDED")
            .filter(|path| !path.is_empty())
            .map(std::path::PathBuf::from);
        let profile = Profile::aggregate(
            &stacks,
            |pc| symbol_name(process, pc, debug),
            folded.is_some(),
        );
        // SAFETY: The DbgHelp session and the process handle have no remaining users.
        unsafe {
            SymCleanup(process);
            CloseHandle(process);
        }
        let _ = profile.write_summary(std::io::stderr().lock(), wall, top);
        if let Some(path) = folded {
            let written = std::fs::File::create_new(&path).and_then(|file| {
                let mut output = std::io::BufWriter::new(file);
                profile.write_folded(&mut output)?;
                output.flush()
            });
            if let Err(error) = written {
                eprintln!(
                    "aevia-prof: cannot write folded profile `{}`: {error}",
                    path.display()
                );
            }
        }
        status
    }
}
