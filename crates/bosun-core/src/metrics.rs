//! System-wide metrics: CPU, RAM, disk activity and network throughput.
//!
//! Every sampler is cheap and allocation-free after construction, so the tray
//! can poll it every second without growing memory.

use std::time::Instant;

use windows::Win32::Foundation::FILETIME;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PdhAddEnglishCounterW, PdhCloseQuery,
    PdhCollectQueryData, PdhGetFormattedCounterValue, PdhOpenQueryW,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;
use windows::core::w;

/// Cumulative CPU times in 100 ns units, as returned by `GetSystemTimes`.
/// `kernel` includes idle time (Windows quirk).
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct CpuTimes {
    pub idle: u64,
    pub kernel: u64,
    pub user: u64,
}

/// Busy percentage between two samples, clamped to 0..=100.
pub fn cpu_percent(prev: CpuTimes, cur: CpuTimes) -> f32 {
    let idle = cur.idle.saturating_sub(prev.idle);
    let total = cur.kernel.saturating_sub(prev.kernel) + cur.user.saturating_sub(prev.user);
    if total == 0 {
        return 0.0;
    }
    let busy = total.saturating_sub(idle);
    (busy as f64 * 100.0 / total as f64).clamp(0.0, 100.0) as f32
}

/// Bytes/second from two cumulative counters. A counter that went backwards
/// (adapter reset) yields 0 rather than a huge spike.
pub fn per_second(prev: u64, cur: u64, secs: f64) -> u64 {
    if secs <= 0.0 || cur < prev {
        return 0;
    }
    ((cur - prev) as f64 / secs) as u64
}

fn ft(f: FILETIME) -> u64 {
    (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
}

pub fn read_cpu_times() -> CpuTimes {
    let (mut i, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: all three out-pointers are valid stack FILETIMEs.
    if unsafe { GetSystemTimes(Some(&mut i), Some(&mut k), Some(&mut u)) }.is_err() {
        return CpuTimes::default();
    }
    CpuTimes {
        idle: ft(i),
        kernel: ft(k),
        user: ft(u),
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Memory {
    pub load_pct: u32,
    pub total: u64,
    pub available: u64,
}

impl Memory {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }
}

pub fn read_memory() -> Memory {
    let mut m = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: dwLength is set as the API requires.
    if unsafe { GlobalMemoryStatusEx(&mut m) }.is_err() {
        return Memory::default();
    }
    Memory {
        load_pct: m.dwMemoryLoad,
        total: m.ullTotalPhys,
        available: m.ullAvailPhys,
    }
}

/// One network adapter's cumulative counters.
#[derive(Clone, Debug)]
pub struct Adapter {
    pub name: String,
    pub rx: u64,
    pub tx: u64,
    pub link_bps: u64,
}

/// Physical, connected adapters only (filters and loopback double-count traffic).
pub fn read_adapters() -> Vec<Adapter> {
    let mut out = Vec::new();
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    // SAFETY: GetIfTable2 allocates the table; we free it with FreeMibTable below.
    if unsafe { GetIfTable2(&mut table) }.is_err() || table.is_null() {
        return out;
    }
    unsafe {
        let t = &*table;
        let rows = std::slice::from_raw_parts(t.Table.as_ptr(), t.NumEntries as usize);
        for r in rows {
            let flags = r.InterfaceAndOperStatusFlags._bitfield;
            let hardware = flags & 0b01 != 0;
            let filter = flags & 0b10 != 0;
            if !hardware || filter || r.OperStatus != IfOperStatusUp {
                continue;
            }
            let len = r
                .Alias
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(r.Alias.len());
            out.push(Adapter {
                name: String::from_utf16_lossy(&r.Alias[..len]),
                rx: r.InOctets,
                tx: r.OutOctets,
                link_bps: r.ReceiveLinkSpeed,
            });
        }
        FreeMibTable(table as *const _);
    }
    out
}

/// Disk "active time" (what Task Manager shows), via a PDH query kept open.
pub struct DiskSampler {
    query: isize,
    counter: isize,
}

impl DiskSampler {
    pub fn new() -> Option<Self> {
        let mut q = Default::default();
        let mut c = Default::default();
        // SAFETY: out-params are valid; the query is closed in Drop.
        unsafe {
            if PdhOpenQueryW(None, 0, &mut q) != 0 {
                return None;
            }
            if PdhAddEnglishCounterW(q, w!("\\PhysicalDisk(_Total)\\% Idle Time"), 0, &mut c) != 0 {
                PdhCloseQuery(q);
                return None;
            }
            PdhCollectQueryData(q);
        }
        Some(Self {
            query: q.0 as isize,
            counter: c.0 as isize,
        })
    }

    /// Busy % since the previous call.
    pub fn sample(&mut self) -> f32 {
        let mut v = PDH_FMT_COUNTERVALUE::default();
        // SAFETY: handles are live for self's lifetime.
        unsafe {
            if PdhCollectQueryData(self.q()) != 0 {
                return 0.0;
            }
            if PdhGetFormattedCounterValue(self.c(), PDH_FMT_DOUBLE, None, &mut v) != 0 {
                return 0.0;
            }
            (100.0 - v.Anonymous.doubleValue).clamp(0.0, 100.0) as f32
        }
    }

    fn q(&self) -> windows::Win32::System::Performance::PDH_HQUERY {
        windows::Win32::System::Performance::PDH_HQUERY(self.query as _)
    }
    fn c(&self) -> windows::Win32::System::Performance::PDH_HCOUNTER {
        windows::Win32::System::Performance::PDH_HCOUNTER(self.counter as _)
    }
}

impl Drop for DiskSampler {
    fn drop(&mut self) {
        // SAFETY: closing the query we opened.
        unsafe { PdhCloseQuery(self.q()) };
    }
}

/// Everything the widget and the monitor header need, refreshed by `tick`.
#[derive(Clone, Copy, Default, Debug)]
pub struct Vitals {
    pub cpu_pct: f32,
    pub mem: Memory,
    pub disk_pct: f32,
    pub rx_bps: u64,
    pub tx_bps: u64,
}

pub struct VitalsSampler {
    cpu: CpuTimes,
    rx: u64,
    tx: u64,
    at: Instant,
    disk: Option<DiskSampler>,
    pub latest: Vitals,
}

impl VitalsSampler {
    pub fn new() -> Self {
        let a = read_adapters();
        Self {
            cpu: read_cpu_times(),
            rx: a.iter().map(|x| x.rx).sum(),
            tx: a.iter().map(|x| x.tx).sum(),
            at: Instant::now(),
            disk: DiskSampler::new(),
            latest: Vitals {
                mem: read_memory(),
                ..Default::default()
            },
        }
    }

    /// `full` also samples disk and network (only needed while the monitor is open).
    pub fn tick(&mut self, full: bool) -> Vitals {
        let now = Instant::now();
        let secs = now.duration_since(self.at).as_secs_f64();
        let cpu = read_cpu_times();
        let mut v = Vitals {
            cpu_pct: cpu_percent(self.cpu, cpu),
            mem: read_memory(),
            ..self.latest
        };
        if full {
            if let Some(d) = self.disk.as_mut() {
                v.disk_pct = d.sample();
            }
            let a = read_adapters();
            let (rx, tx) = (a.iter().map(|x| x.rx).sum(), a.iter().map(|x| x.tx).sum());
            v.rx_bps = per_second(self.rx, rx, secs);
            v.tx_bps = per_second(self.tx, tx, secs);
            self.rx = rx;
            self.tx = tx;
        }
        self.cpu = cpu;
        self.at = now;
        self.latest = v;
        v
    }
}

impl Default for VitalsSampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percent_math() {
        let a = CpuTimes {
            idle: 100,
            kernel: 200,
            user: 100,
        };
        // 100 kernel(incl. 50 idle) + 100 user = 200 total, 50 idle -> 75% busy
        let b = CpuTimes {
            idle: 150,
            kernel: 300,
            user: 200,
        };
        assert!((cpu_percent(a, b) - 75.0).abs() < 0.01);
        assert_eq!(cpu_percent(a, a), 0.0);
        // counters going backwards never produce garbage
        assert_eq!(cpu_percent(b, a), 0.0);
    }

    #[test]
    fn per_second_handles_resets() {
        assert_eq!(per_second(1000, 3000, 2.0), 1000);
        assert_eq!(per_second(3000, 1000, 1.0), 0);
        assert_eq!(per_second(0, 10, 0.0), 0);
    }

    #[test]
    fn live_readings_are_sane() {
        let m = read_memory();
        assert!(m.total > 0 && m.load_pct <= 100);
        let mut s = VitalsSampler::new();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let v = s.tick(true);
        assert!((0.0..=100.0).contains(&v.cpu_pct));
        assert!((0.0..=100.0).contains(&v.disk_pct));
    }
}
