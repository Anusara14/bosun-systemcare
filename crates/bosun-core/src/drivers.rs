//! Driver updates offered by Windows Update (spec §2.2: no third-party driver
//! installer). Searching can take a minute, so callers run it off the UI
//! thread; installing is done by Windows Update itself (Settings → Optional
//! updates), which we open.

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::UpdateAgent::{IUpdateSession, IWindowsDriverUpdate, UpdateSession};
use windows::core::{BSTR, Interface};

#[derive(Clone, Debug, PartialEq)]
pub struct Driver {
    pub title: String,
    /// Device class ("Display", "Bluetooth", …).
    pub class: String,
    pub provider: String,
    /// Driver date, `YYYY-MM-DD`.
    pub date: String,
}

/// OLE automation date (days since 1899-12-30) → `YYYY-MM-DD`.
pub fn ole_date(d: f64) -> String {
    if !(1.0..2_958_465.0).contains(&d) {
        return String::new();
    }
    // days since 1970-01-01, then the civil-from-days algorithm
    let days = d.floor() as i64 - 25_569;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Driver updates Windows Update offers this PC (not yet installed).
pub fn search() -> windows::core::Result<Vec<Driver>> {
    // SAFETY: COM calls on objects created here; strings are copied out.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let session: IUpdateSession = CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER)?;
        let searcher = session.CreateUpdateSearcher()?;
        let result = searcher.Search(&BSTR::from("IsInstalled=0 and Type='Driver'"))?;
        let updates = result.Updates()?;
        let mut out = Vec::new();
        for i in 0..updates.Count()? {
            let u = updates.get_Item(i)?;
            let d = u.cast::<IWindowsDriverUpdate>().ok();
            let s = |r: windows::core::Result<BSTR>| r.map(|b| b.to_string()).unwrap_or_default();
            out.push(Driver {
                title: s(u.Title()),
                class: d.as_ref().map(|d| s(d.DriverClass())).unwrap_or_default(),
                provider: d
                    .as_ref()
                    .map(|d| s(d.DriverProvider()))
                    .unwrap_or_default(),
                date: d
                    .as_ref()
                    .and_then(|d| d.DriverVerDate().ok())
                    .map(ole_date)
                    .unwrap_or_default(),
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ole_dates_convert() {
        assert_eq!(ole_date(25_569.0), "1970-01-01");
        assert_eq!(ole_date(45_000.5), "2023-03-15");
        assert_eq!(ole_date(0.0), "");
    }

    /// Asks Windows Update (slow, network): `cargo test -p bosun-core real_drivers -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_drivers() {
        match search() {
            Ok(v) => {
                for d in &v {
                    println!("{} | {} | {} | {}", d.title, d.class, d.provider, d.date);
                }
                println!("{} driver updates", v.len());
            }
            Err(e) => println!("search failed: {e}"),
        }
    }
}
