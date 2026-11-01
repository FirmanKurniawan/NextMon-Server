//! Thin Windows helpers: job objects, process queries, registry, named mutex, console attach, listening-port owner.
#![allow(clippy::missing_safety_doc)]

#[cfg(windows)]
pub use imp::*;
#[cfg(not(windows))]
pub use fallback::*;

#[cfg(windows)]
mod imp {
    use crate::util::wide;
    use std::ffi::{c_void, OsStr};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::Console::*;
    use windows_sys::Win32::System::JobObjects::*;
    use windows_sys::Win32::System::Registry::*;
    use windows_sys::Win32::System::Threading::*;

    /// Job object that kills every assigned process when the handle closes (i.e. when our exe exits for any reason).
    pub struct JobObject(HANDLE);
    unsafe impl Send for JobObject {}
    unsafe impl Sync for JobObject {}

    impl JobObject {
        pub fn new_kill_on_close() -> Option<JobObject> {
            unsafe {
                let h = CreateJobObjectW(null(), null());
                if h.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    CloseHandle(h);
                    return None;
                }
                Some(JobObject(h))
            }
        }
        pub fn assign_raw(&self, process: *mut c_void) -> bool {
            unsafe { AssignProcessToJobObject(self.0, process as HANDLE) != 0 }
        }
        pub fn terminate(&self) {
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }
    impl Drop for JobObject {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub fn is_alive(pid: u32) -> bool {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code);
            CloseHandle(h);
            ok != 0 && code == 259 // STILL_ACTIVE
        }
    }

    pub fn image_path(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut buf = vec![0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(h);
            if ok == 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    pub fn kill_pid(pid: u32) -> bool {
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if h.is_null() {
                return false;
            }
            let ok = TerminateProcess(h, 1);
            CloseHandle(h);
            ok != 0
        }
    }

    /// Working set in MB (0 if unavailable).
    pub fn working_set_mb(pid: u32) -> f64 {
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return 0.0;
            }
            let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            let ok = K32GetProcessMemoryInfo(h, &mut c, c.cb);
            CloseHandle(h);
            if ok == 0 {
                return 0.0;
            }
            ((c.WorkingSetSize as f64 / 1048576.0) * 10.0).round() / 10.0
        }
    }

    /// PID listening on a local IPv4 TCP port (None if free or unknown).
    pub fn port_owner(port: u16) -> Option<u32> {
        use windows_sys::Win32::NetworkManagement::IpHelper::*;
        unsafe {
            let mut size = 0u32;
            GetExtendedTcpTable(null_mut(), &mut size, 0, 2, TCP_TABLE_OWNER_PID_LISTENER, 0);
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize + 1024];
            size = buf.len() as u32;
            if GetExtendedTcpTable(buf.as_mut_ptr() as *mut c_void, &mut size, 0, 2, TCP_TABLE_OWNER_PID_LISTENER, 0) != 0 {
                return None;
            }
            let table = buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID;
            let n = (*table).dwNumEntries as usize;
            let rows = std::slice::from_raw_parts((*table).table.as_ptr(), n);
            for r in rows {
                let p = u16::from_be((r.dwLocalPort & 0xffff) as u16);
                if p == port {
                    return Some(r.dwOwningPid);
                }
            }
            None
        }
    }

    pub fn process_name(pid: u32) -> Option<String> {
        image_path(pid).and_then(|p| p.rsplit(['\\', '/']).next().map(|s| s.to_string()))
    }

    fn hkey(root: &str) -> HKEY {
        match root {
            "HKLM" => HKEY_LOCAL_MACHINE,
            _ => HKEY_CURRENT_USER,
        }
    }

    pub fn reg_get_string(root: &str, subkey: &str, name: &str) -> Option<String> {
        unsafe {
            let sk = wide(OsStr::new(subkey));
            let nm = wide(OsStr::new(name));
            let mut size = 0u32;
            if RegGetValueW(hkey(root), sk.as_ptr(), nm.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), &mut size) != 0 {
                return None;
            }
            let mut buf = vec![0u16; (size as usize / 2) + 2];
            size = (buf.len() * 2) as u32;
            if RegGetValueW(hkey(root), sk.as_ptr(), nm.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr() as *mut c_void, &mut size) != 0 {
                return None;
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..end]))
        }
    }

    pub fn reg_get_dword(root: &str, subkey: &str, name: &str) -> Option<u32> {
        unsafe {
            let sk = wide(OsStr::new(subkey));
            let nm = wide(OsStr::new(name));
            let mut v = 0u32;
            let mut size = 4u32;
            if RegGetValueW(hkey(root), sk.as_ptr(), nm.as_ptr(), RRF_RT_REG_DWORD, null_mut(), &mut v as *mut u32 as *mut c_void, &mut size) != 0 {
                return None;
            }
            Some(v)
        }
    }

    pub fn reg_set_string(root: &str, subkey: &str, name: &str, value: &str) -> bool {
        unsafe {
            let sk = wide(OsStr::new(subkey));
            let nm = wide(OsStr::new(name));
            let val = wide(OsStr::new(value));
            RegSetKeyValueW(hkey(root), sk.as_ptr(), nm.as_ptr(), REG_SZ, val.as_ptr() as *const c_void, (val.len() * 2) as u32) == 0
        }
    }

    pub fn reg_delete_value(root: &str, subkey: &str, name: &str) -> bool {
        unsafe {
            let sk = wide(OsStr::new(subkey));
            let nm = wide(OsStr::new(name));
            let r = RegDeleteKeyValueW(hkey(root), sk.as_ptr(), nm.as_ptr());
            r == 0 || r == 2 // ERROR_FILE_NOT_FOUND counts as already deleted
        }
    }

    /// Named mutex held for the process lifetime. Returns (handle_keepalive, already_running).
    pub fn single_instance(name: &str) -> (usize, bool) {
        unsafe {
            let n = wide(OsStr::new(name));
            let h = CreateMutexW(null(), 1, n.as_ptr());
            let already = GetLastError() == 183; // ERROR_ALREADY_EXISTS
            (h as usize, already)
        }
    }

    /// Windows-subsystem exe started from a console: attach to it so CLI output is visible.
    pub fn attach_parent_console() {
        unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                return;
            }
            for (std_id, dev, access) in [
                (STD_OUTPUT_HANDLE, "CONOUT$", 0x4000_0000u32),
                (STD_ERROR_HANDLE, "CONOUT$", 0x4000_0000u32),
                (STD_INPUT_HANDLE, "CONIN$", 0x8000_0000u32),
            ] {
                let cur = GetStdHandle(std_id);
                if cur.is_null() || cur == INVALID_HANDLE_VALUE {
                    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING, FILE_SHARE_READ, FILE_SHARE_WRITE};
                    let d = wide(OsStr::new(dev));
                    let h = CreateFileW(d.as_ptr(), access, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut());
                    if h != INVALID_HANDLE_VALUE {
                        SetStdHandle(std_id, h);
                    }
                }
            }
        }
    }

    pub fn raise_window_hint() {}
}

#[cfg(not(windows))]
mod fallback {
    pub struct JobObject;
    impl JobObject {
        pub fn new_kill_on_close() -> Option<JobObject> {
            None
        }
        pub fn assign_raw(&self, _p: *mut std::ffi::c_void) -> bool {
            false
        }
        pub fn terminate(&self) {}
    }
    pub fn is_alive(_pid: u32) -> bool {
        false
    }
    pub fn image_path(_pid: u32) -> Option<String> {
        None
    }
    pub fn kill_pid(_pid: u32) -> bool {
        false
    }
    pub fn working_set_mb(_pid: u32) -> f64 {
        0.0
    }
    pub fn port_owner(_port: u16) -> Option<u32> {
        None
    }
    pub fn process_name(_pid: u32) -> Option<String> {
        None
    }
    pub fn reg_get_string(_r: &str, _s: &str, _n: &str) -> Option<String> {
        None
    }
    pub fn reg_get_dword(_r: &str, _s: &str, _n: &str) -> Option<u32> {
        None
    }
    pub fn reg_set_string(_r: &str, _s: &str, _n: &str, _v: &str) -> bool {
        false
    }
    pub fn reg_delete_value(_r: &str, _s: &str, _n: &str) -> bool {
        false
    }
    pub fn single_instance(_n: &str) -> (usize, bool) {
        (0, false)
    }
    pub fn attach_parent_console() {}
    pub fn raise_window_hint() {}
}
