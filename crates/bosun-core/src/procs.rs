//! Process snapshots for the Performance Monitor: one `NtQuerySystemInformation`
//! call gives every process's private memory, CPU time and I/O without opening
//! each process (cheap, and works for elevated processes too).

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::time::Instant;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, TerminateProcess,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongW, GetWindowThreadProcessId,
    IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, HSTRING, PWSTR};

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(class: u32, buf: *mut c_void, len: u32, ret: *mut u32) -> i32;
}

const SYSTEM_PROCESS_INFORMATION_CLASS: u32 = 5;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;

/// x64 layout of `SYSTEM_PROCESS_INFORMATION` (fields Microsoft documents as
/// "Reserved" are named here; layout is stable since Windows Vista).
#[repr(C)]
struct SysProc {
    next: u32,
    threads: u32,
    private_ws: i64,
    _hard_faults: u32,
    _threads_hwm: u32,
    _cycle_time: u64,
    create_time: i64,
    user_time: i64,
    kernel_time: i64,
    name_len: u16,
    _name_max: u16,
    name_buf: *const u16,
    _base_priority: i32,
    pid: usize,
    _parent: usize,
    _handles: u32,
    _session: u32,
    _key: usize,
    _peak_vsize: usize,
    _vsize: usize,
    _faults: u32,
    _peak_ws: usize,
    _ws: usize,
    _q1: usize,
    _q2: usize,
    _q3: usize,
    _q4: usize,
    _pagefile: usize,
    _peak_pagefile: usize,
    _private_pages: usize,
    _read_ops: i64,
    _write_ops: i64,
    _other_ops: i64,
    read_bytes: i64,
    write_bytes: i64,
    other_bytes: i64,
}

/// One process as read from the kernel, before grouping.
#[derive(Clone, Debug)]
pub struct Raw {
    pub pid: u32,
    pub create: i64,
    pub image: String,
    pub private_bytes: u64,
    pub cpu_time: u64,
    pub io_bytes: u64,
}

/// What the monitor shows: all processes of one executable merged into a row.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub key: String,
    pub name: String,
    pub path: Option<String>,
    pub pids: Vec<u32>,
    pub private_bytes: u64,
    pub cpu_pct: f32,
    pub io_bps: u64,
    pub has_window: bool,
    pub protected: bool,
}

/// Ending these takes Windows down or is pointless; the UI disables their checkbox.
const PROTECTED: &[&str] = &[
    "system",
    "registry",
    "memory compression",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "svchost.exe",
    "dwm.exe",
    "fontdrvhost.exe",
    "sihost.exe",
    "ctfmon.exe",
    "secure system",
];

pub fn is_protected(image: &str, pid: u32) -> bool {
    pid <= 4 || pid == std::process::id() || PROTECTED.contains(&image.to_lowercase().as_str())
}

/// Per-pid CPU and I/O deltas → grouped, sorted rows. Pure, so it is tested
/// without touching the system.
pub fn group(
    raw: &[Raw],
    prev: &HashMap<(u32, i64), (u64, u64)>,
    secs: f64,
    ncpu: u32,
    path_of: &dyn Fn(u32, i64) -> Option<String>,
    windowed: &HashSet<u32>,
) -> Vec<Group> {
    let mut by_key: HashMap<String, Group> = HashMap::new();
    let ticks = secs * 10_000_000.0 * f64::from(ncpu.max(1));
    for p in raw {
        let (cpu, io) = match prev.get(&(p.pid, p.create)) {
            Some(&(c0, i0)) if secs > 0.0 => (
                (p.cpu_time.saturating_sub(c0) as f64 * 100.0 / ticks) as f32,
                (p.io_bytes.saturating_sub(i0) as f64 / secs) as u64,
            ),
            _ => (0.0, 0),
        };
        let path = path_of(p.pid, p.create);
        let key = path.as_deref().unwrap_or(&p.image).to_lowercase();
        let g = by_key.entry(key.clone()).or_insert_with(|| Group {
            key,
            name: p.image.clone(),
            path: path.clone(),
            pids: Vec::new(),
            private_bytes: 0,
            cpu_pct: 0.0,
            io_bps: 0,
            has_window: false,
            protected: false,
        });
        g.pids.push(p.pid);
        g.private_bytes += p.private_bytes;
        g.cpu_pct += cpu;
        g.io_bps += io;
        g.has_window |= windowed.contains(&p.pid);
        g.protected |= is_protected(&p.image, p.pid);
    }
    let mut v: Vec<Group> = by_key.into_values().collect();
    for g in &mut v {
        g.cpu_pct = g.cpu_pct.clamp(0.0, 100.0);
    }
    v.sort_by(|a, b| {
        b.private_bytes
            .cmp(&a.private_bytes)
            .then(a.key.cmp(&b.key))
    });
    v
}

/// Keeps buffers and caches between refreshes so steady-state polling does not allocate much.
pub struct ProcessTable {
    buf: Vec<u64>,
    prev: HashMap<(u32, i64), (u64, u64)>,
    paths: HashMap<(u32, i64), Option<String>>,
    names: HashMap<String, String>,
    at: Instant,
    ncpu: u32,
}

impl Default for ProcessTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessTable {
    pub fn new() -> Self {
        Self {
            buf: vec![0; 64 * 1024],
            prev: HashMap::new(),
            paths: HashMap::new(),
            names: HashMap::new(),
            at: Instant::now(),
            ncpu: std::thread::available_parallelism().map_or(1, |n| n.get() as u32),
        }
    }

    /// Reads every process; `None` only if the kernel call fails outright.
    pub fn read_raw(&mut self) -> Option<Vec<Raw>> {
        loop {
            let bytes = (self.buf.len() * 8) as u32;
            let mut needed = 0u32;
            // SAFETY: buf is a live, 8-byte aligned allocation of `bytes` bytes.
            let st = unsafe {
                NtQuerySystemInformation(
                    SYSTEM_PROCESS_INFORMATION_CLASS,
                    self.buf.as_mut_ptr().cast(),
                    bytes,
                    &mut needed,
                )
            };
            if st == STATUS_INFO_LENGTH_MISMATCH {
                let want = (needed as usize / 8) + 4096; // headroom: processes start meanwhile
                self.buf.resize(want.max(self.buf.len() * 2), 0);
                continue;
            }
            if st < 0 {
                return None;
            }
            break;
        }
        let base = self.buf.as_ptr().cast::<u8>();
        let mut off = 0usize;
        let mut out = Vec::with_capacity(400);
        loop {
            // SAFETY: the kernel filled a chain of SysProc records; `next` offsets stay
            // inside the buffer and each record is 8-byte aligned.
            let p = unsafe { &*base.add(off).cast::<SysProc>() };
            let image = if p.name_buf.is_null() {
                if p.pid == 0 {
                    "System Idle Process".into()
                } else {
                    String::new()
                }
            } else {
                // SAFETY: name_buf points at name_len bytes of UTF-16 inside the buffer.
                let s = unsafe { std::slice::from_raw_parts(p.name_buf, p.name_len as usize / 2) };
                String::from_utf16_lossy(s)
            };
            if p.pid != 0 {
                out.push(Raw {
                    pid: p.pid as u32,
                    create: p.create_time,
                    image,
                    private_bytes: p.private_ws.max(0) as u64,
                    cpu_time: (p.user_time + p.kernel_time).max(0) as u64,
                    io_bytes: (p.read_bytes + p.write_bytes + p.other_bytes).max(0) as u64,
                });
            }
            if p.next == 0 {
                break;
            }
            off += p.next as usize;
        }
        Some(out)
    }

    /// Fresh grouped rows. Call about once a second while the monitor is open.
    pub fn refresh(&mut self) -> Vec<Group> {
        let Some(raw) = self.read_raw() else {
            return Vec::new();
        };
        let now = Instant::now();
        let secs = now.duration_since(self.at).as_secs_f64();

        let live: HashSet<(u32, i64)> = raw.iter().map(|r| (r.pid, r.create)).collect();
        self.paths.retain(|k, _| live.contains(k));
        for r in &raw {
            self.paths
                .entry((r.pid, r.create))
                .or_insert_with(|| image_path(r.pid));
        }
        let paths = &self.paths;
        let windowed = windowed_pids();
        let mut groups = group(
            &raw,
            &self.prev,
            secs,
            self.ncpu,
            &|pid, ct| paths.get(&(pid, ct)).cloned().flatten(),
            &windowed,
        );
        for g in &mut groups {
            if let Some(path) = &g.path {
                let n = self
                    .names
                    .entry(path.clone())
                    .or_insert_with(|| file_description(path).unwrap_or_else(|| g.name.clone()));
                g.name.clone_from(n);
            }
        }
        self.prev = raw
            .iter()
            .map(|r| ((r.pid, r.create), (r.cpu_time, r.io_bytes)))
            .collect();
        self.at = now;
        groups
    }

    /// Drops caches and shrinks buffers; call when the monitor closes.
    pub fn release(&mut self) {
        self.buf = vec![0; 64 * 1024];
        self.prev = HashMap::new();
        self.paths = HashMap::new();
        self.names = HashMap::new();
    }
}

/// Full Win32 path of a process's executable, if we are allowed to ask.
pub fn image_path(pid: u32) -> Option<String> {
    // SAFETY: handle is closed before returning; buffer length is passed in/out.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// The friendly name Windows shows in Task Manager ("Google Chrome").
pub fn file_description(path: &str) -> Option<String> {
    let hpath = HSTRING::from(path);
    // SAFETY: version data buffer is sized by GetFileVersionInfoSizeW and outlives
    // the pointers VerQueryValueW returns into it.
    unsafe {
        let size = GetFileVersionInfoSizeW(&hpath, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&hpath, None, size, data.as_mut_ptr().cast()).ok()?;
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let tr = HSTRING::from("\\VarFileInfo\\Translation");
        if !VerQueryValueW(data.as_ptr().cast(), &tr, &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let lang = *ptr.cast::<u16>();
        let cp = *ptr.cast::<u16>().add(1);
        let q = HSTRING::from(format!(
            "\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"
        ));
        if !VerQueryValueW(data.as_ptr().cast(), &q, &mut ptr, &mut len).as_bool() || len <= 1 {
            return None;
        }
        let s = std::slice::from_raw_parts(ptr.cast::<u16>(), len as usize);
        let s = String::from_utf16_lossy(s)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        (!s.is_empty()).then_some(s)
    }
}

/// PIDs that own a visible top-level app window (what users think of as "apps").
pub fn windowed_pids() -> HashSet<u32> {
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        // SAFETY: lp is the &mut HashSet passed below, alive for the EnumWindows call.
        unsafe {
            let set = &mut *(lp.0 as *mut HashSet<u32>);
            let tool = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0;
            let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|h| !h.is_invalid());
            if IsWindowVisible(hwnd).as_bool() && !tool && !owned {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                set.insert(pid);
            }
        }
        true.into()
    }
    let mut set = HashSet::new();
    // SAFETY: callback only runs during this call while `set` is alive.
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut set as *mut _ as isize));
    }
    set
}

/// Asks Windows to trim every accessible process's working set ("Clean RAM").
/// Returns bytes of physical memory that became available. Honest caveat: apps
/// page back what they actually use, so the gain is temporary.
pub fn trim_working_sets() -> u64 {
    use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows::Win32::System::Threading::PROCESS_SET_QUOTA;
    let before = crate::metrics::read_memory().available;
    let mut t = ProcessTable::new();
    for p in t.read_raw().unwrap_or_default() {
        if is_protected(&p.image, p.pid) {
            continue;
        }
        // SAFETY: handle closed right after use.
        unsafe {
            if let Ok(h) = OpenProcess(
                PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                p.pid,
            ) {
                let _ = EmptyWorkingSet(h);
                let _ = CloseHandle(h);
            }
        }
    }
    crate::metrics::read_memory()
        .available
        .saturating_sub(before)
}

/// Ends every process in `pids`. Returns how many could not be ended
/// (elevated or protected processes, or ones that already exited).
pub fn end_task(pids: &[u32]) -> usize {
    pids.iter()
        .filter(|&&pid| {
            // SAFETY: handle closed right after use.
            unsafe {
                match OpenProcess(PROCESS_TERMINATE, false, pid) {
                    Ok(h) => {
                        let r = TerminateProcess(h, 1);
                        let _ = CloseHandle(h);
                        r.is_err()
                    }
                    Err(_) => true,
                }
            }
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(pid: u32, image: &str, mem: u64, cpu: u64, io: u64) -> Raw {
        Raw {
            pid,
            create: 1,
            image: image.into(),
            private_bytes: mem,
            cpu_time: cpu,
            io_bytes: io,
        }
    }

    #[test]
    fn groups_by_exe_and_sorts_by_memory() {
        let r = vec![
            raw(10, "chrome.exe", 100, 0, 0),
            raw(11, "chrome.exe", 50, 0, 0),
            raw(20, "notepad.exe", 120, 0, 0),
        ];
        let g = group(
            &r,
            &HashMap::new(),
            1.0,
            4,
            &|_, _| None,
            &HashSet::from([20]),
        );
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].key, "chrome.exe");
        assert_eq!(g[0].private_bytes, 150);
        assert_eq!(g[0].pids, vec![10, 11]);
        assert!(g[1].has_window && !g[0].has_window);
    }

    #[test]
    fn cpu_and_io_come_from_deltas() {
        let r = vec![raw(10, "a.exe", 1, 20_000_000, 3000)];
        let prev = HashMap::from([((10, 1), (10_000_000, 1000))]);
        // 1 s on 2 cores = 20M ticks available; 10M used -> 50 %
        let g = group(&r, &prev, 1.0, 2, &|_, _| None, &HashSet::new());
        assert!((g[0].cpu_pct - 50.0).abs() < 0.01);
        assert_eq!(g[0].io_bps, 2000);
    }

    #[test]
    fn protects_system_processes() {
        assert!(is_protected("csrss.exe", 600));
        assert!(is_protected("anything", 4));
        assert!(!is_protected("chrome.exe", 1234));
    }

    #[test]
    fn live_snapshot_contains_this_process() {
        let mut t = ProcessTable::new();
        let raw = t.read_raw().expect("kernel snapshot");
        let me = std::process::id();
        let mine = raw.iter().find(|r| r.pid == me).expect("own pid present");
        assert!(mine.private_bytes > 0);
        assert!(!t.refresh().is_empty());
    }
}
