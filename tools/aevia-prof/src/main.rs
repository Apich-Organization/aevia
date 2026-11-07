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
//! `AEVIA_PROF_INTERVAL_US` (sampling interval, default 1000).

#[cfg(not(windows))]
fn main() {
    eprintln!("aevia-prof only supports Windows; use `perf` elsewhere");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    std::process::exit(windows::run());
}

#[cfg(windows)]
#[allow(unsafe_code, clippy::too_many_lines)]
mod windows {
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::ffi::c_void;
    use std::time::Duration;
    use std::time::Instant;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Diagnostics::Debug::*;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::Threading::*;

    #[repr(C, align(16))]
    struct AlignedContext(CONTEXT);

    const MAX_FRAMES: usize = 192;
    const IMAGE_FILE_MACHINE_AMD64: u32 = 0x8664;
    const CONTEXT_AMD64: u32 = 0x0010_0000;
    const CONTEXT_CONTROL_AMD64: u32 = CONTEXT_AMD64 | 0x1;
    const CONTEXT_INTEGER_AMD64: u32 = CONTEXT_AMD64 | 0x2;

    struct Symbols {
        process: HANDLE,
        names: HashMap<u64, String>,
    }

    impl Symbols {
        fn name(
            &mut self,
            address: u64,
        ) -> String {
            if let Some(name) = self.names.get(&address) {
                return name.clone();
            }
            const NAME_LEN: usize = 1024;
            let mut buffer = vec![0u8; std::mem::size_of::<SYMBOL_INFO>() + NAME_LEN];
            let symbol = buffer.as_mut_ptr().cast::<SYMBOL_INFO>();
            let name = unsafe {
                (*symbol).SizeOfStruct = std::mem::size_of::<SYMBOL_INFO>() as u32;
                (*symbol).MaxNameLen = NAME_LEN as u32;
                let mut displacement = 0u64;
                if SymFromAddr(self.process, address, &mut displacement, symbol) != 0 {
                    let len = (*symbol).NameLen as usize;
                    let ptr = std::ptr::addr_of!((*symbol).Name).cast::<u8>();
                    let bytes = std::slice::from_raw_parts(ptr, len.min(NAME_LEN));
                    simplify(&String::from_utf8_lossy(bytes))
                } else {
                    if std::env::var_os("AEVIA_PROF_DEBUG").is_some() {
                        eprintln!(
                            "SymFromAddr 0x{address:x} failed: {}",
                            windows_sys::Win32::Foundation::GetLastError()
                        );
                    }
                    format!("0x{address:x}")
                }
            };
            self.names.insert(address, name.clone());
            name
        }
    }

    /// Drops generic arguments and hash suffixes so one function is one row.
    fn simplify(name: &str) -> String {
        let mut out = String::with_capacity(name.len());
        let mut depth = 0usize;
        for ch in name.chars() {
            match ch {
                | '<' => {
                    if depth == 0 {
                        out.push_str("<…>");
                    }
                    depth += 1;
                },
                | '>' => depth = depth.saturating_sub(1),
                | _ if depth == 0 => out.push(ch),
                | _ => {},
            }
        }
        out
    }

    fn main_module_base(pid: u32) -> u64 {
        main_module(pid).0
    }

    fn main_module(pid: u32) -> (u64, u64) {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, pid);
            if snapshot.is_null() || snapshot as isize == -1 {
                return (0, 0);
            }
            let mut entry: MODULEENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<MODULEENTRY32>() as u32;
            let module = if Module32First(snapshot, &mut entry) != 0 {
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
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot.is_null() || snapshot as isize == -1 {
                return out;
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            if Thread32First(snapshot, &mut entry) != 0 {
                loop {
                    if entry.th32OwnerProcessID == pid {
                        out.push(entry.th32ThreadID);
                    }
                    if Thread32Next(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
        }
        out
    }

    fn sample_thread(
        process: HANDLE,
        thread: HANDLE,
    ) -> Option<Vec<u64>> {
        unsafe {
            if SuspendThread(thread) == u32::MAX {
                return None;
            }
            let mut context: AlignedContext = std::mem::zeroed();
            context.0.ContextFlags = CONTEXT_CONTROL_AMD64 | CONTEXT_INTEGER_AMD64;
            let mut frames = Vec::new();
            if GetThreadContext(thread, &mut context.0) != 0 {
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
                        &mut frame,
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

    fn idle(leaf: &str) -> bool {
        [
            "Wait",
            "ZwDelay",
            "NtDelay",
            "Sleep",
            "ZwRemoveIo",
            "NtRemoveIo",
            "RtlUserThreadStart",
        ]
        .iter()
        .any(|needle| leaf.contains(needle))
    }

    pub fn run() -> i32 {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() {
            eprintln!("usage: aevia-prof <program> [args...]");
            return 2;
        }
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
        unsafe {
            windows_sys::Win32::Media::timeBeginPeriod(1);
        }
        let mut child = match std::process::Command::new(&args[0])
            .args(&args[1..])
            .spawn()
        {
            | Ok(child) => child,
            | Err(error) => {
                eprintln!("aevia-prof: cannot start {}: {error}", args[0]);
                return 2;
            },
        };
        let pid = child.id();
        let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if process.is_null() {
            eprintln!("aevia-prof: cannot open the child process");
            let _ = child.wait();
            return 2;
        }
        std::thread::sleep(Duration::from_millis(30));
        let debug = std::env::var_os("AEVIA_PROF_DEBUG").is_some();
        unsafe {
            SymSetOptions(SYMOPT_UNDNAME);
            // Load the main image explicitly at its actual (ASLR) base once
            // the loader has mapped it; system modules are not symbolized.
            let mut base = 0;
            for _ in 0..200 {
                base = main_module_base(pid);
                if base != 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let image_path = std::fs::canonicalize(&args[0])
                .map(|path| {
                    let text = path.display().to_string();
                    text.strip_prefix(r"\?\")
                        .map_or(text.clone(), str::to_string)
                })
                .unwrap_or_else(|_| args[0].replace('/', "\\"));
            let directory = std::path::Path::new(&image_path)
                .parent()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default();
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
                info.SizeOfStruct = std::mem::size_of::<IMAGEHLP_MODULE64>() as u32;
                let got = SymGetModuleInfo64(process, base, &mut info);
                let pdb = String::from_utf8_lossy(
                    &info
                        .LoadedPdbName
                        .iter()
                        .map(|&c| c as u8)
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
        let in_image = |pc: u64| pc >= image_base && pc < image_base + image_size;
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
                let handle = *handles.entry(tid).or_insert_with(|| unsafe {
                    OpenThread(
                        THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
                        0,
                        tid,
                    )
                });
                if handle.is_null() {
                    continue;
                }
                if let Some(mut frames) = sample_thread(process, handle) {
                    // A thread parked in the system (the CLI's main thread
                    // joining the interpreter thread) has no frame in the
                    // image; frames past the image's outermost one are the
                    // unsymbolized thread start.
                    let Some(last) = frames.iter().rposition(|&pc| in_image(pc)) else {
                        continue;
                    };
                    frames.truncate(last + 1);
                    // System frames on top (heap calls) are kept as one leaf.
                    *stacks.entry(frames).or_default() += 1;
                }
            }
            std::thread::sleep(interval);
        };
        let wall = started.elapsed();
        for handle in handles.values() {
            if !handle.is_null() {
                unsafe {
                    CloseHandle(*handle);
                }
            }
        }
        let mut symbols = Symbols {
            process,
            names: HashMap::new(),
        };
        let mut self_counts: HashMap<String, u64> = HashMap::new();
        let mut total_counts: HashMap<String, u64> = HashMap::new();
        let mut samples = 0u64;
        for (frames, count) in &stacks {
            let names: Vec<String> = frames.iter().map(|&pc| symbols.name(pc)).collect();
            if names.first().is_none_or(|leaf| idle(leaf)) {
                continue;
            }
            samples += count;
            *self_counts.entry(names[0].clone()).or_default() += count;
            let mut seen = HashSet::new();
            for name in names {
                if seen.insert(name.clone()) {
                    *total_counts.entry(name).or_default() += count;
                }
            }
        }
        unsafe {
            SymCleanup(process);
            CloseHandle(process);
        }
        let print = |title: &str, counts: &HashMap<String, u64>| {
            let mut rows: Vec<_> = counts.iter().collect();
            rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
            eprintln!(
                "\n== {title} ({samples} busy samples, {:.2}s wall)",
                wall.as_secs_f64()
            );
            for (name, count) in rows.into_iter().take(top) {
                let pct = 100.0 * (*count as f64) / (samples.max(1) as f64);
                let mut name = name.clone();
                if name.len() > 150 {
                    name.truncate(150);
                }
                eprintln!("{pct:6.2}% {count:7}  {name}");
            }
        };
        print("self", &self_counts);
        print("inclusive", &total_counts);
        status
    }
}
