//! Bosun's own updates, from the project's GitHub releases: find the latest
//! release, download its installer, check it against the published SHA-256,
//! and run it with `--update` (it closes Bosun, replaces the files and starts
//! Bosun again).
//!
//! The network work goes through Windows' own `curl.exe` (System32, Windows
//! 10 1803+), so there's no HTTP stack in our binaries.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "Anusara14/bosun-systemcare";
/// Every download must come from this repo's release files.
fn download_prefix() -> String {
    format!("https://github.com/{REPO}/releases/download/")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: (u32, u32, u32),
    pub setup_url: String,
    pub sha_url: String,
}

impl Release {
    pub fn label(&self) -> String {
        let (a, b, c) = self.version;
        format!("{a}.{b}.{c}")
    }

    /// Newer than the running build.
    pub fn is_newer(&self) -> bool {
        parse_version(env!("CARGO_PKG_VERSION")).is_some_and(|me| self.version > me)
    }
}

/// "v1.2.3" or "1.2.3" (anything after a '-' or '+' is ignored).
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let core = s.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u32>().ok());
    let v = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(v)
}

/// The installer and its checksum from GitHub's "latest release" JSON.
pub fn parse_release(json: &str) -> io::Result<Release> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| bad(&e.to_string()))?;
    let version = v["tag_name"]
        .as_str()
        .and_then(parse_version)
        .ok_or_else(|| bad("the release has no version tag"))?;
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    let url = |pred: &dyn Fn(&str) -> bool| {
        assets.iter().find_map(|a| {
            let name = a["name"].as_str()?;
            let url = a["browser_download_url"].as_str()?;
            (pred(name) && url.starts_with(&download_prefix())).then(|| url.to_string())
        })
    };
    let is_setup = |n: &str| n.starts_with("BosunSystemCare-Setup-") && n.ends_with(".exe");
    let setup_url = url(&is_setup).ok_or_else(|| bad("the release has no installer"))?;
    let sha_url = url(&|n: &str| n.ends_with(".exe.sha256") && is_setup(&n[..n.len() - 7]))
        .ok_or_else(|| bad("the release has no checksum"))?;
    Ok(Release {
        version,
        setup_url,
        sha_url,
    })
}

fn curl(args: &[&str]) -> io::Result<Vec<u8>> {
    use std::os::windows::process::CommandExt;
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    let out = Command::new(PathBuf::from(root).join(r"System32\curl.exe"))
        // https only, redirects included; fail on HTTP errors
        .args(["-fsSL", "--proto", "=https", "--max-time", "300"])
        .args([
            "-A",
            concat!("Bosun-SystemCare/", env!("CARGO_PKG_VERSION")),
        ])
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(io::Error::other(if msg.is_empty() {
            "couldn't reach GitHub".into()
        } else {
            msg
        }));
    }
    Ok(out.stdout)
}

/// The latest published release.
pub fn latest() -> io::Result<Release> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let json = curl(&["-H", "Accept: application/vnd.github+json", &url])?;
    parse_release(&String::from_utf8_lossy(&json))
}

pub fn sha256(data: &[u8]) -> io::Result<[u8; 32]> {
    use windows::Win32::Security::Cryptography::{BCRYPT_SHA256_ALG_HANDLE, BCryptHash};
    let mut out = [0u8; 32];
    // SAFETY: one-shot hash with the built-in SHA-256 provider into a 32-byte buffer.
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) }
        .ok()
        .map_err(io::Error::other)?;
    Ok(out)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Downloads the installer to `%TEMP%\Bosun-update` and checks its SHA-256
/// against the release's `.sha256` file. Returns the installer's path.
pub fn download(r: &Release) -> io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("Bosun-update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let name = r.setup_url.rsplit('/').next().unwrap_or("setup.exe");
    let path = dir.join(name);
    let expected = String::from_utf8_lossy(&curl(&[&r.sha_url])?)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    curl(&["-o", &path.to_string_lossy(), &r.setup_url])?;
    let actual = hex(&sha256(&std::fs::read(&path)?)?);
    if expected.len() != 64 || actual != expected {
        let _ = std::fs::remove_file(&path);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the download didn't match its checksum",
        ));
    }
    Ok(path)
}

/// Where this copy is installed (the folder with `uninstall.exe`), if it is.
fn install_dir() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    dir.join("uninstall.exe").is_file().then_some(dir)
}

/// Starts the verified installer in update mode. It closes the running app
/// and tray, so the caller should exit soon after. `open_app` starts the main
/// window again when it's done.
pub fn install(setup: &Path, open_app: bool) -> io::Result<()> {
    let mut c = Command::new(setup);
    c.arg("--update");
    if open_app {
        c.arg("--open");
    }
    // a development build installs a normal copy instead of overwriting itself
    if let Some(d) = install_dir() {
        c.arg("--dir").arg(d);
    }
    c.spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v0.10.2"), Some((0, 10, 2)));
        assert_eq!(parse_version("1.2.3-beta"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert!((0, 10, 0) > (0, 9, 9));
    }

    #[test]
    fn reads_the_release_json_and_rejects_foreign_urls() {
        let base = "https://github.com/Anusara14/bosun-systemcare/releases/download/v0.3.0";
        let json = format!(
            r#"{{"tag_name":"v0.3.0","assets":[
              {{"name":"BosunSystemCare-Setup-0.3.0.exe","browser_download_url":"{base}/BosunSystemCare-Setup-0.3.0.exe"}},
              {{"name":"BosunSystemCare-Setup-0.3.0.exe.sha256","browser_download_url":"{base}/BosunSystemCare-Setup-0.3.0.exe.sha256"}}]}}"#
        );
        let r = parse_release(&json).unwrap();
        assert_eq!(r.label(), "0.3.0");
        assert!(r.setup_url.ends_with("0.3.0.exe") && r.sha_url.ends_with(".sha256"));
        let evil = json.replace("https://github.com/Anusara14", "https://evil.example/x");
        assert!(parse_release(&evil).is_err());
    }

    /// Network: `cargo test -p bosun-core live_release -- --ignored`
    #[test]
    #[ignore]
    fn live_release_downloads_and_verifies() {
        let r = latest().unwrap();
        assert!(r.version >= (0, 2, 0), "{r:?}");
        let p = download(&r).unwrap();
        assert!(std::fs::metadata(&p).unwrap().len() > 1_000_000);
    }

    #[test]
    fn hashes_with_sha256() {
        assert_eq!(
            hex(&sha256(b"abc").unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
