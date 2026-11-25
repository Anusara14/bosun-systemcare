//! Startup impact (spec §2.2): what each startup program cost at sign-in,
//! from the measurements Windows keeps for Task Manager's "Startup impact"
//! (`%WINDIR%\System32\wdi\LogFiles\StartupInfo\*.xml`, one file per user
//! sign-in). That folder is admin-only, so an elevated `bosun.exe
//! --measure-startup <file>` reads it once and writes a summary the
//! unelevated window can read; nothing elevated keeps running.

use std::io;
use std::path::{Path, PathBuf};

/// One program's cost at the measured sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Measured {
    /// Image name, lower case (`onedrive.exe`).
    pub exe: String,
    pub cpu_ms: u64,
    pub disk_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Impact {
    Low,
    Medium,
    High,
}

impl Impact {
    /// Task Manager's thresholds: High above 1 s of CPU or 3 MB of disk,
    /// Medium above 300 ms or 300 KB.
    pub fn of(m: &Measured) -> Impact {
        if m.cpu_ms > 1000 || m.disk_bytes > 3 << 20 {
            Impact::High
        } else if m.cpu_ms > 300 || m.disk_bytes > 300 << 10 {
            Impact::Medium
        } else {
            Impact::Low
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Impact::Low => "Low",
            Impact::Medium => "Medium",
            Impact::High => "High",
        }
    }
}

/// The text between `<tag …>` and `</tag>` in `block`, and the tag's
/// `Unit` attribute if any.
fn element<'a>(block: &'a str, tag: &str) -> Option<(&'a str, Option<&'a str>)> {
    let open = block.find(&format!("<{tag}"))?;
    let head_end = open + block[open..].find('>')?;
    let head = &block[open..head_end];
    let unit = head
        .find("Unit=\"")
        .and_then(|i| head[i + 6..].split('"').next());
    let body_end = head_end + block[head_end..].find(&format!("</{tag}>"))?;
    Some((block[head_end + 1..body_end].trim(), unit))
}

fn attr<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    let i = head.find(&format!("{name}=\""))?;
    head[i + name.len() + 2..].split('"').next()
}

/// Programs in one StartupInfo document, summed per image name.
pub fn parse(xml: &str) -> Vec<Measured> {
    let mut out: Vec<Measured> = Vec::new();
    for block in xml.split("<Process ").skip(1) {
        let Some(name) = block.split('>').next().and_then(|h| attr(h, "Name")) else {
            continue;
        };
        let exe = name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(name)
            .to_lowercase();
        let num = |tag: &str| -> u64 {
            element(block, tag).map_or(0, |(v, unit)| {
                let n: f64 = v.parse().unwrap_or(0.0);
                match unit.map(str::to_ascii_lowercase).as_deref() {
                    Some("us") => (n / 1000.0) as u64,
                    Some("s") => (n * 1000.0) as u64,
                    _ => n as u64, // ms, bytes
                }
            })
        };
        let (cpu_ms, disk_bytes) = (num("CpuUsage"), num("DiskUsage"));
        match out.iter_mut().find(|m| m.exe == exe) {
            Some(m) => {
                m.cpu_ms += cpu_ms;
                m.disk_bytes += disk_bytes;
            }
            None => out.push(Measured {
                exe,
                cpu_ms,
                disk_bytes,
            }),
        }
    }
    out
}

/// Text of a StartupInfo file (UTF-16 with a byte-order mark, or UTF-8).
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let u: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&u)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Where the summary lives for this user.
pub fn cache_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join(r"Bosun\startup-impact.tsv"))
}

/// Elevated: reads the newest StartupInfo file and writes the summary to
/// `out`. Returns how many programs were measured.
// ponytail: the newest file is the latest sign-in, normally this user's;
// per-SID matching if several people share the PC
pub fn measure_into(out: &Path) -> io::Result<usize> {
    let windir = std::env::var_os("WINDIR").ok_or_else(|| io::Error::other("WINDIR is unset"))?;
    let dir = PathBuf::from(windir).join(r"System32\wdi\LogFiles\StartupInfo");
    let newest = std::fs::read_dir(&dir)?
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("xml"))
        })
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no startup measurements yet"))?;
    let measured = parse(&decode(&std::fs::read(newest.path())?));
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut text = format!("# measured {secs}\n");
    for m in &measured {
        text.push_str(&format!("{}\t{}\t{}\n", m.exe, m.cpu_ms, m.disk_bytes));
    }
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(out, text)?;
    Ok(measured.len())
}

/// Why the last measuring run failed, if it did.
pub fn last_error() -> Option<String> {
    let p = cache_path()?.with_extension("err");
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
}

/// The saved summary: (when measured, unix secs; programs).
pub fn load() -> Option<(u64, Vec<Measured>)> {
    let text = std::fs::read_to_string(cache_path()?).ok()?;
    let mut lines = text.lines();
    let at = lines
        .next()?
        .strip_prefix("# measured ")?
        .trim()
        .parse()
        .ok()?;
    let v = lines
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some(Measured {
                exe: f.next()?.to_string(),
                cpu_ms: f.next()?.parse().ok()?,
                disk_bytes: f.next()?.parse().ok()?,
            })
        })
        .collect();
    Some((at, v))
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-16"?>
<StartupData>
  <Process Name="C:\Program Files\Microsoft OneDrive\OneDrive.exe" PID="1234" StartTime="2026-11-24T08:00:01Z">
    <ParentPID>900</ParentPID>
    <CommandLine>"C:\...\OneDrive.exe" /background</CommandLine>
    <DiskUsage Unit="bytes">5242880</DiskUsage>
    <CpuUsage Unit="us">1500000</CpuUsage>
  </Process>
  <Process Name="Zebar.exe" PID="1300" StartTime="2026-11-24T08:00:02Z">
    <DiskUsage Unit="bytes">1024</DiskUsage>
    <CpuUsage Unit="us">40000</CpuUsage>
  </Process>
  <Process Name="zebar.exe" PID="1301" StartTime="2026-11-24T08:00:03Z">
    <DiskUsage Unit="bytes">1024</DiskUsage>
    <CpuUsage Unit="us">10000</CpuUsage>
  </Process>
</StartupData>"#;

    #[test]
    fn reads_task_managers_measurements_and_grades_them() {
        let v = parse(XML);
        assert_eq!(v.len(), 2, "same image name summed");
        assert_eq!(
            v[0],
            Measured {
                exe: "onedrive.exe".into(),
                cpu_ms: 1500,
                disk_bytes: 5_242_880
            }
        );
        assert_eq!((v[1].exe.as_str(), v[1].cpu_ms), ("zebar.exe", 50));
        assert_eq!(Impact::of(&v[0]), Impact::High);
        assert_eq!(Impact::of(&v[1]), Impact::Low);
        let mid = Measured {
            exe: "x".into(),
            cpu_ms: 400,
            disk_bytes: 0,
        };
        assert_eq!(Impact::of(&mid), Impact::Medium);
    }

    #[test]
    fn decodes_utf16_files() {
        let mut b = vec![0xFF, 0xFE];
        b.extend(
            "<Process Name=\"a.exe\">"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        assert!(decode(&b).contains("a.exe"));
    }
}
