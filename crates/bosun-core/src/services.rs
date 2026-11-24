//! Services (spec §2.2, Startup Optimizer → Services): non-Windows services
//! by default, with how they start. Changing the start type needs admin; the
//! old type is journaled first so it can be put back.

use std::io::{self, Write};
use std::path::Path;

use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ENUM_SERVICE_STATUS_PROCESSW, ENUM_SERVICE_TYPE,
    EnumServicesStatusExW, OpenSCManagerW, OpenServiceW, QUERY_SERVICE_CONFIGW,
    QueryServiceConfig2W, QueryServiceConfigW, SC_ENUM_PROCESS_INFO, SC_HANDLE,
    SC_MANAGER_ENUMERATE_SERVICE, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG,
    SERVICE_CONFIG_DESCRIPTION, SERVICE_DEMAND_START, SERVICE_DESCRIPTIONW, SERVICE_DISABLED,
    SERVICE_ERROR, SERVICE_NO_CHANGE, SERVICE_QUERY_CONFIG, SERVICE_RUNNING, SERVICE_START_TYPE,
    SERVICE_STATE_ALL, SERVICE_WIN32,
};
use windows::core::{HSTRING, PCWSTR};

use crate::paths;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Start {
    Automatic,
    Manual,
    Disabled,
    /// Boot/system drivers and anything else we don't offer to change.
    Other,
}

impl Start {
    fn from(t: SERVICE_START_TYPE) -> Self {
        match t {
            SERVICE_AUTO_START => Start::Automatic,
            SERVICE_DEMAND_START => Start::Manual,
            SERVICE_DISABLED => Start::Disabled,
            _ => Start::Other,
        }
    }

    fn raw(self) -> Option<SERVICE_START_TYPE> {
        match self {
            Start::Automatic => Some(SERVICE_AUTO_START),
            Start::Manual => Some(SERVICE_DEMAND_START),
            Start::Disabled => Some(SERVICE_DISABLED),
            Start::Other => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Start::Automatic => "Automatic",
            Start::Manual => "Manual",
            Start::Disabled => "Disabled",
            Start::Other => "System",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Service {
    /// Internal name (what the service manager uses).
    pub name: String,
    pub display: String,
    pub description: String,
    pub start: Start,
    pub running: bool,
    /// The service's program, when recognisable.
    pub exe: Option<std::path::PathBuf>,
    /// Part of Windows: its program lives under `%WINDIR%`.
    pub system: bool,
}

struct Handle(SC_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: closing a handle we opened.
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn wide(p: windows::core::PWSTR) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: NUL-terminated string inside a buffer we own.
        unsafe { p.to_string().unwrap_or_default() }
    }
}

fn config(scm: &Handle, name: &str) -> Option<(SERVICE_START_TYPE, String, String)> {
    // SAFETY: query calls into buffers sized by the first call.
    unsafe {
        let s = Handle(OpenServiceW(scm.0, &HSTRING::from(name), SERVICE_QUERY_CONFIG).ok()?);
        let mut need = 0u32;
        let _ = QueryServiceConfigW(s.0, None, 0, &mut need);
        let mut buf = vec![0u64; (need as usize).div_ceil(8).max(1)];
        let cfg = buf.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
        QueryServiceConfigW(s.0, Some(cfg), (buf.len() * 8) as u32, &mut need).ok()?;
        let start = (*cfg).dwStartType;
        let path = wide((*cfg).lpBinaryPathName);
        let mut need = 0u32;
        let _ = QueryServiceConfig2W(s.0, SERVICE_CONFIG_DESCRIPTION, None, &mut need);
        let mut dbuf = vec![0u64; (need as usize).div_ceil(8).max(1)];
        let bytes = std::slice::from_raw_parts_mut(dbuf.as_mut_ptr().cast::<u8>(), dbuf.len() * 8);
        let desc = if QueryServiceConfig2W(s.0, SERVICE_CONFIG_DESCRIPTION, Some(bytes), &mut need)
            .is_ok()
        {
            wide((*dbuf.as_ptr().cast::<SERVICE_DESCRIPTIONW>()).lpDescription)
        } else {
            String::new()
        };
        Some((start, path, desc))
    }
}

/// Every Win32 service, Windows' own marked `system`.
pub fn list() -> Vec<Service> {
    let mut out = Vec::new();
    // SAFETY: enumeration into a buffer sized by the first call.
    unsafe {
        let Ok(scm) = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ENUMERATE_SERVICE)
        else {
            return out;
        };
        let scm = Handle(scm);
        let (mut need, mut count) = (0u32, 0u32);
        let _ = EnumServicesStatusExW(
            scm.0,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_STATE_ALL,
            None,
            &mut need,
            &mut count,
            None,
            PCWSTR::null(),
        );
        let mut buf = vec![0u64; (need as usize).div_ceil(8) + 64];
        let bytes = std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), buf.len() * 8);
        if EnumServicesStatusExW(
            scm.0,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_STATE_ALL,
            Some(bytes),
            &mut need,
            &mut count,
            None,
            PCWSTR::null(),
        )
        .is_err()
        {
            return out;
        }
        let entries = std::slice::from_raw_parts(
            buf.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
            count as usize,
        );
        let win = std::env::var("WINDIR")
            .unwrap_or_default()
            .to_ascii_lowercase();
        for e in entries {
            let name = wide(e.lpServiceName);
            let Some((start, path, description)) = config(&scm, &name) else {
                continue;
            };
            let exe = paths::exe_from_command(&path);
            let system = exe
                .as_ref()
                .is_none_or(|p| p.to_string_lossy().to_ascii_lowercase().starts_with(&win));
            out.push(Service {
                display: wide(e.lpDisplayName),
                name,
                description,
                start: Start::from(start),
                running: e.ServiceStatusProcess.dwCurrentState == SERVICE_RUNNING,
                exe,
                system,
            });
        }
    }
    out.sort_by_key(|s| s.display.to_lowercase());
    out
}

/// Changes how a service starts, journaling the old type first. Needs admin.
pub fn set_start(svc: &Service, to: Start, dir: &Path) -> io::Result<()> {
    let raw = to
        .raw()
        .ok_or_else(|| io::Error::other("can't set that start type"))?;
    let (_, mut f) = crate::clean::journal_file(dir, "Services")?;
    writeln!(f, "V\t{}\t{}\t{}", svc.name, svc.start.label(), to.label())?;
    f.sync_all()?;
    let no = SERVICE_NO_CHANGE;
    // SAFETY: opens our target service and changes only its start type.
    unsafe {
        let scm = Handle(OpenSCManagerW(
            PCWSTR::null(),
            PCWSTR::null(),
            SC_MANAGER_ENUMERATE_SERVICE,
        )?);
        let s = Handle(OpenServiceW(
            scm.0,
            &HSTRING::from(svc.name.as_str()),
            SERVICE_CHANGE_CONFIG,
        )?);
        ChangeServiceConfigW(
            s.0,
            ENUM_SERVICE_TYPE(no),
            raw,
            SERVICE_ERROR(no),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_windows_services_and_marks_them_system() {
        let all = list();
        // Every Windows install has the Event Log service, run by svchost.
        let ev = all
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case("EventLog"))
            .unwrap();
        assert!(ev.system && ev.running);
        assert_eq!(ev.start, Start::Automatic);
        assert!(!ev.description.is_empty());
    }

    /// Read-only: `cargo test -p bosun-core real_services -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_services() {
        let all = list();
        for s in all.iter().filter(|s| !s.system) {
            println!(
                "{:<9} run={} {} | {}",
                s.start.label(),
                s.running,
                s.display,
                s.name
            );
        }
        println!(
            "{} services, {} non-Windows",
            all.len(),
            all.iter().filter(|s| !s.system).count()
        );
    }
}
