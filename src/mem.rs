use std::ffi::c_void;
use std::mem::{offset_of, size_of, zeroed};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::Debug::{
    FlushInstructionCache, ReadProcessMemory, WriteProcessMemory,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, PROCESSENTRY32W,
    Process32FirstW, Process32NextW, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ,
    PAGE_EXECUTE_READWRITE, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY, VirtualAllocEx,
    VirtualFreeEx, VirtualProtectEx, VirtualQueryEx,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    QueryFullProcessImageNameW, WaitForSingleObject,
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateRemoteThread(
        hProcess: HANDLE,
        lpThreadAttributes: *const c_void,
        dwStackSize: usize,
        lpStartAddress: unsafe extern "system" fn(*mut c_void) -> u32,
        lpParameter: *mut c_void,
        dwCreationFlags: u32,
        lpThreadId: *mut u32,
    ) -> HANDLE;
}

const PROCESS_ACCESS: u32 = PROCESS_QUERY_INFORMATION
    | PROCESS_QUERY_LIMITED_INFORMATION
    | PROCESS_VM_READ
    | PROCESS_VM_WRITE
    | PROCESS_VM_OPERATION
    | PROCESS_CREATE_THREAD;

const STILL_ACTIVE: u32 = 259;
const WAIT_TIMEOUT: u32 = 0x102;
const WAIT_OBJECT_0: u32 = 0;

const REMOTE_CALL_STUB: &[u8] = &[
    0x53, 0x48, 0x89, 0xCB, 0x48, 0x8B, 0x03, 0x48, 0x8B, 0x4B, 0x08, 0x48, 0x8B, 0x53, 0x10, 0x4C,
    0x8B, 0x43, 0x18, 0x4C, 0x8B, 0x4B, 0x20, 0x48, 0x83, 0xEC, 0x20, 0xFF, 0xD0, 0x48, 0x83, 0xC4,
    0x20, 0x48, 0x89, 0x43, 0x28, 0x31, 0xC0, 0x5B, 0xC3,
];

const REMOTE_MONO_COMPILE_STUB: &[u8] = &[
    0x53, 0x48, 0x89, 0xCB, 0x48, 0x8B, 0x03, 0x48, 0x83, 0xEC, 0x20, 0xFF, 0xD0, 0x48, 0x83, 0xC4,
    0x20, 0x48, 0x85, 0xC0, 0x74, 0x48, 0x48, 0x89, 0x43, 0x38, 0x48, 0x89, 0xC1, 0x48, 0x8B, 0x43,
    0x08, 0x48, 0x83, 0xEC, 0x20, 0xFF, 0xD0, 0x48, 0x83, 0xC4, 0x20, 0x48, 0x8B, 0x43, 0x10, 0x48,
    0x85, 0xC0, 0x74, 0x10, 0x48, 0x8B, 0x4B, 0x38, 0x31, 0xD2, 0x48, 0x83, 0xEC, 0x20, 0xFF, 0xD0,
    0x48, 0x83, 0xC4, 0x20, 0x48, 0x8B, 0x4B, 0x20, 0x48, 0x8B, 0x43, 0x18, 0x48, 0x83, 0xEC, 0x20,
    0xFF, 0xD0, 0x48, 0x83, 0xC4, 0x20, 0x48, 0x89, 0x43, 0x28, 0x31, 0xC0, 0x5B, 0xC3, 0x31, 0xC0,
    0x48, 0x89, 0x43, 0x28, 0x5B, 0xC3,
];

#[repr(C)]
struct RemoteCallLayout {
    func: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    result: u64,
}

#[repr(C)]
struct MonoCompileLayout {
    get_root_domain: u64,
    thread_attach: u64,
    domain_set: u64,
    compile_method: u64,
    method: u64,
    result: u64,
    _pad: u64,
    domain_tmp: u64,
}

#[derive(Debug)]
pub enum OpenError {
    NotFound { name: String },
    Multiple { name: String, count: usize },
    AccessDenied { pid: u32 },
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { name } => write!(f, "process not found: {name}"),
            Self::Multiple { name, count } => {
                write!(
                    f,
                    "found {count} processes named {name}; close the extras and retry"
                )
            }
            Self::AccessDenied { pid } => {
                write!(f, "OpenProcess({pid}) failed — run as Administrator?")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ModuleInfo {
    pub base: u64,
}

pub struct Process {
    pub(crate) handle: HANDLE,
    pid: u32,
    pending_remote_calls: Mutex<Vec<PendingRemoteCall>>,
}

unsafe impl Send for Process {}

struct PendingRemoteCall {
    thread: HANDLE,
    code: u64,
    data: u64,
}

impl Drop for Process {
    fn drop(&mut self) {
        if let Ok(pending) = self.pending_remote_calls.get_mut() {
            for call in pending.drain(..) {
                if unsafe { WaitForSingleObject(call.thread, 0) } == WAIT_OBJECT_0 {
                    unsafe {
                        VirtualFreeEx(self.handle, call.data as *mut c_void, 0, MEM_RELEASE);
                        VirtualFreeEx(self.handle, call.code as *mut c_void, 0, MEM_RELEASE);
                    }
                }
                close_handle(call.thread);
            }
        }
        close_handle(self.handle);
    }
}

impl Process {
    pub fn open_by_name(name: &str) -> Result<Self, OpenError> {
        let pids = find_pids(name);
        let pid = match pids.as_slice() {
            [] => {
                return Err(OpenError::NotFound {
                    name: name.to_string(),
                });
            }
            [pid] => *pid,
            _ => {
                return Err(OpenError::Multiple {
                    name: name.to_string(),
                    count: pids.len(),
                });
            }
        };
        Self::open_pid(pid)
    }

    pub fn open_pid(pid: u32) -> Result<Self, OpenError> {
        let handle = unsafe { OpenProcess(PROCESS_ACCESS, FALSE, pid) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(OpenError::AccessDenied { pid });
        }
        Ok(Self {
            handle,
            pid,
            pending_remote_calls: Mutex::new(Vec::new()),
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn exe_path(&self) -> Option<std::path::PathBuf> {
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(self.handle, 0, buf.as_mut_ptr(), &mut size) };
        (ok != 0 && size > 0).then(|| {
            let s = String::from_utf16_lossy(&buf[..size as usize]);
            std::path::PathBuf::from(s)
        })
    }

    pub fn read_bytes(&self, addr: u64, buf: &mut [u8]) -> bool {
        if addr == 0 || buf.is_empty() {
            return false;
        }
        let mut n = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                self.handle,
                addr as *const c_void,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut n,
            )
        };
        ok != 0 && n == buf.len()
    }

    pub fn write_bytes(&self, addr: u64, buf: &[u8]) -> bool {
        if addr == 0 || buf.is_empty() {
            return false;
        }
        let mut n = 0usize;
        let ok = unsafe {
            WriteProcessMemory(
                self.handle,
                addr as *mut c_void,
                buf.as_ptr().cast(),
                buf.len(),
                &mut n,
            )
        };
        ok != 0 && n == buf.len()
    }

    pub fn write_code_bytes(&self, addr: u64, buf: &[u8]) -> bool {
        if addr == 0 || buf.is_empty() {
            return false;
        }
        let mut old = 0u32;
        let ok = unsafe {
            VirtualProtectEx(
                self.handle,
                addr as *mut c_void,
                buf.len(),
                PAGE_EXECUTE_READWRITE,
                &mut old,
            )
        };
        if ok == 0 {
            return false;
        }
        let written = self.write_bytes(addr, buf);
        let mut ignored = 0u32;
        unsafe {
            VirtualProtectEx(
                self.handle,
                addr as *mut c_void,
                buf.len(),
                old,
                &mut ignored,
            );
        }
        written
    }

    fn read_le<const N: usize>(&self, addr: u64) -> Option<[u8; N]> {
        let mut b = [0u8; N];
        self.read_bytes(addr, &mut b).then_some(b)
    }

    pub fn read_u64(&self, addr: u64) -> Option<u64> {
        self.read_le(addr).map(u64::from_le_bytes)
    }

    pub fn read_u32(&self, addr: u64) -> Option<u32> {
        self.read_le(addr).map(u32::from_le_bytes)
    }

    pub fn read_u16(&self, addr: u64) -> Option<u16> {
        self.read_le(addr).map(u16::from_le_bytes)
    }

    pub fn read_f32(&self, addr: u64) -> Option<f32> {
        self.read_le(addr).map(f32::from_le_bytes)
    }

    pub fn read_c_string(&self, addr: u64, max_len: usize) -> Option<String> {
        if addr == 0 || max_len == 0 {
            return None;
        }
        let mut buf = vec![0u8; max_len];
        if self.read_bytes(addr, &mut buf) {
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            return Some(String::from_utf8_lossy(&buf[..len]).into_owned());
        }
        let mut acc = Vec::new();
        for i in 0..max_len {
            let Some(b) = self.read_le::<1>(addr + i as u64) else {
                break;
            };
            if b[0] == 0 {
                return Some(String::from_utf8_lossy(&acc).into_owned());
            }
            acc.push(b[0]);
        }
        (!acc.is_empty()).then(|| String::from_utf8_lossy(&acc).into_owned())
    }

    pub fn write_f32(&self, addr: u64, v: f32) -> bool {
        self.write_bytes(addr, &v.to_le_bytes())
    }

    pub fn write_u32(&self, addr: u64, v: u32) -> bool {
        self.write_bytes(addr, &v.to_le_bytes())
    }

    pub fn write_u64(&self, addr: u64, v: u64) -> bool {
        self.write_bytes(addr, &v.to_le_bytes())
    }

    pub fn alloc_remote(&self, size: usize) -> Option<u64> {
        self.alloc_remote_protect(size, PAGE_READWRITE)
    }

    pub fn alloc_remote_protect(&self, size: usize, protect: u32) -> Option<u64> {
        if size == 0 {
            return None;
        }
        let p = unsafe {
            VirtualAllocEx(
                self.handle,
                std::ptr::null(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                protect,
            )
        };
        (!p.is_null()).then_some(p as u64)
    }

    pub fn free_remote(&self, addr: u64) {
        if addr == 0 {
            return;
        }
        unsafe {
            VirtualFreeEx(self.handle, addr as *mut c_void, 0, MEM_RELEASE);
        }
    }

    pub fn module_by_name(&self, name: &str) -> Option<ModuleInfo> {
        let want = name.to_ascii_lowercase();
        let snap = Snapshot::modules(self.pid)?;
        let mut me = unsafe { zeroed::<MODULEENTRY32W>() };
        me.dwSize = size_of::<MODULEENTRY32W>() as u32;
        unsafe {
            if Module32FirstW(snap.0, &mut me) == 0 {
                return None;
            }
            loop {
                if widestr(&me.szModule).eq_ignore_ascii_case(&want) {
                    return Some(ModuleInfo {
                        base: me.modBaseAddr as u64,
                    });
                }
                if Module32NextW(snap.0, &mut me) == 0 {
                    break;
                }
            }
        }
        None
    }

    pub fn alloc_remote_string(&self, s: &str) -> Option<u64> {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        let addr = self.alloc_remote(bytes.len())?;
        if self.write_bytes(addr, &bytes) {
            Some(addr)
        } else {
            self.free_remote(addr);
            None
        }
    }

    pub fn remote_call(&self, func: u64, args: &[u64], timeout_ms: u32) -> Result<u64, String> {
        if func == 0 {
            return Err("null function".into());
        }
        if args.len() > 4 {
            return Err("at most 4 args".into());
        }

        let mut a = [0u64; 4];
        a[..args.len()].copy_from_slice(args);
        let layout = RemoteCallLayout {
            func,
            arg0: a[0],
            arg1: a[1],
            arg2: a[2],
            arg3: a[3],
            result: 0,
        };
        let layout_bytes = unsafe {
            std::slice::from_raw_parts(
                (&layout as *const RemoteCallLayout).cast::<u8>(),
                size_of::<RemoteCallLayout>(),
            )
        };
        let out = self.run_remote_stub(
            REMOTE_CALL_STUB,
            layout_bytes,
            offset_of!(RemoteCallLayout, result),
            timeout_ms,
        )?;
        Ok(out)
    }

    pub fn remote_mono_compile(
        &self,
        get_root_domain: u64,
        thread_attach: u64,
        domain_set: u64,
        compile_method: u64,
        method: u64,
        timeout_ms: u32,
    ) -> Result<u64, String> {
        if get_root_domain == 0 || thread_attach == 0 || compile_method == 0 || method == 0 {
            return Err("null mono compile arg".into());
        }
        let layout = MonoCompileLayout {
            get_root_domain,
            thread_attach,
            domain_set,
            compile_method,
            method,
            result: 0,
            _pad: 0,
            domain_tmp: 0,
        };
        let layout_bytes = unsafe {
            std::slice::from_raw_parts(
                (&layout as *const MonoCompileLayout).cast::<u8>(),
                size_of::<MonoCompileLayout>(),
            )
        };
        self.run_remote_stub(
            REMOTE_MONO_COMPILE_STUB,
            layout_bytes,
            offset_of!(MonoCompileLayout, result),
            timeout_ms,
        )
    }

    fn run_remote_stub(
        &self,
        stub: &[u8],
        layout_bytes: &[u8],
        result_off: usize,
        timeout_ms: u32,
    ) -> Result<u64, String> {
        self.reap_pending_remote_calls()?;

        let code = self
            .alloc_remote(stub.len())
            .ok_or("VirtualAllocEx code allocation failed")?;
        let data = match self.alloc_remote(layout_bytes.len()) {
            Some(data) => data,
            None => {
                self.free_remote(code);
                return Err("VirtualAllocEx data allocation failed".into());
            }
        };
        if !self.write_bytes(code, stub) {
            self.free_remote(data);
            self.free_remote(code);
            return Err("write stub failed".into());
        }
        if !self.protect_remote(code, stub.len(), PAGE_EXECUTE_READ) {
            self.free_remote(data);
            self.free_remote(code);
            return Err("protect stub failed".into());
        }
        if unsafe { FlushInstructionCache(self.handle, code as *const c_void, stub.len()) } == 0 {
            self.free_remote(data);
            self.free_remote(code);
            return Err("FlushInstructionCache failed".into());
        }
        if !self.write_bytes(data, layout_bytes) {
            self.free_remote(data);
            self.free_remote(code);
            return Err("write layout failed".into());
        }

        let thread = unsafe {
            CreateRemoteThread(
                self.handle,
                std::ptr::null(),
                0,
                std::mem::transmute::<u64, unsafe extern "system" fn(*mut c_void) -> u32>(code),
                data as *mut c_void,
                0,
                std::ptr::null_mut(),
            )
        };
        if thread.is_null() {
            self.free_remote(data);
            self.free_remote(code);
            return Err("CreateRemoteThread failed".into());
        }

        let w = unsafe { WaitForSingleObject(thread, timeout_ms) };
        if w == WAIT_TIMEOUT {
            self.retain_pending_remote_call(thread, code, data);
            return Err("timed out; the remote call is still running".into());
        }
        if w != WAIT_OBJECT_0 {
            self.retain_pending_remote_call(thread, code, data);
            return Err(format!("WaitForSingleObject failed code={w}"));
        }
        close_handle(thread);

        let result = match self.read_u64(data + result_off as u64) {
            Some(v) => v,
            None => {
                self.free_remote(data);
                self.free_remote(code);
                return Err("read result failed".into());
            }
        };
        self.free_remote(data);
        self.free_remote(code);
        Ok(result)
    }

    fn protect_remote(&self, addr: u64, size: usize, protect: u32) -> bool {
        let mut old = 0u32;
        unsafe { VirtualProtectEx(self.handle, addr as *mut c_void, size, protect, &mut old) != 0 }
    }

    fn reap_pending_remote_calls(&self) -> Result<(), String> {
        let mut pending = self
            .pending_remote_calls
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let mut i = 0;
        while i < pending.len() {
            if unsafe { WaitForSingleObject(pending[i].thread, 0) } == WAIT_OBJECT_0 {
                let call = pending.swap_remove(i);
                close_handle(call.thread);
                self.free_remote(call.data);
                self.free_remote(call.code);
            } else {
                i += 1;
            }
        }
        if pending.is_empty() {
            Ok(())
        } else {
            Err("a previous remote call is still running; retry after it finishes".into())
        }
    }

    fn retain_pending_remote_call(&self, thread: HANDLE, code: u64, data: u64) {
        let mut pending = self
            .pending_remote_calls
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        pending.push(PendingRemoteCall { thread, code, data });
    }

    pub fn is_alive(&self) -> Option<bool> {
        let mut code = 0u32;
        (unsafe { GetExitCodeProcess(self.handle, &mut code) } != 0).then_some(code == STILL_ACTIVE)
    }

    fn regions_matching(&self, matches: impl Fn(u32) -> bool) -> Vec<(u64, u64)> {
        let mut out = Vec::new();
        let mut addr = 0x10000u64;
        let max = 0x0000_7FFF_FFFF_FFFF;
        while addr < max {
            let mut mbi = unsafe { zeroed::<MEMORY_BASIC_INFORMATION>() };
            let n = unsafe {
                VirtualQueryEx(
                    self.handle,
                    addr as *const c_void,
                    &mut mbi,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if n == 0 {
                break;
            }
            let base = mbi.BaseAddress as u64;
            let size = mbi.RegionSize as u64;
            if mbi.State == MEM_COMMIT && matches(mbi.Protect) && size > 0 {
                out.push((base, size));
            }
            let next = base.saturating_add(size);
            if next <= addr {
                break;
            }
            addr = next;
        }
        out
    }

    pub fn readable_regions(&self) -> Vec<(u64, u64)> {
        self.regions_matching(|prot| {
            matches!(
                prot,
                PAGE_READONLY
                    | PAGE_READWRITE
                    | PAGE_WRITECOPY
                    | PAGE_EXECUTE_READ
                    | PAGE_EXECUTE_READWRITE
            )
        })
    }

    pub fn writable_regions(&self) -> Vec<(u64, u64)> {
        self.regions_matching(|prot| {
            matches!(
                prot,
                PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE
            )
        })
    }

    pub fn looks_like_user_ptr(&self, p: u64) -> bool {
        (0x10000..=0x0000_7FFF_FFFF_FFFF).contains(&p)
            && p.is_multiple_of(8)
            && self.read_le::<8>(p).is_some()
    }
}

struct Snapshot(HANDLE);

impl Snapshot {
    fn new(flags: u32, pid: u32) -> Option<Self> {
        let h = unsafe { CreateToolhelp32Snapshot(flags, pid) };
        (h != INVALID_HANDLE_VALUE).then_some(Self(h))
    }

    fn processes() -> Option<Self> {
        Self::new(TH32CS_SNAPPROCESS, 0)
    }

    fn modules(pid: u32) -> Option<Self> {
        Self::new(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        close_handle(self.0);
    }
}

fn close_handle(h: HANDLE) {
    if !h.is_null() && h != INVALID_HANDLE_VALUE {
        unsafe {
            CloseHandle(h);
        }
    }
}

fn find_pids(name: &str) -> Vec<u32> {
    let name_l = name.to_ascii_lowercase();
    let Some(snap) = Snapshot::processes() else {
        return Vec::new();
    };
    let mut pe = unsafe { zeroed::<PROCESSENTRY32W>() };
    pe.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    let mut pids = Vec::new();
    unsafe {
        if Process32FirstW(snap.0, &mut pe) == 0 {
            return pids;
        }
        loop {
            if widestr(&pe.szExeFile).eq_ignore_ascii_case(&name_l) {
                pids.push(pe.th32ProcessID);
            }
            if Process32NextW(snap.0, &mut pe) == 0 {
                break;
            }
        }
    }
    pids
}

fn widestr(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
