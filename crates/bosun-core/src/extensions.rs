//! Browser extensions (spec §2.2, Startup Optimizer → Browser Startups) for
//! Edge, Chrome, Brave, Vivaldi, Opera and Firefox, read from the profiles.
//!
//! Removing is left to the browser itself: we open its page for that
//! extension. Deleting extension folders behind a running browser can corrupt
//! its profile, and the browser re-syncs them anyway.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Browser {
    Edge,
    Chrome,
    Brave,
    Vivaldi,
    Opera,
    Firefox,
}

impl Browser {
    pub fn label(self) -> &'static str {
        match self {
            Browser::Edge => "Microsoft Edge",
            Browser::Chrome => "Google Chrome",
            Browser::Brave => "Brave",
            Browser::Vivaldi => "Vivaldi",
            Browser::Opera => "Opera",
            Browser::Firefox => "Mozilla Firefox",
        }
    }

    /// Program name ShellExecute resolves through App Paths.
    pub fn exe(self) -> &'static str {
        match self {
            Browser::Edge => "msedge.exe",
            Browser::Chrome => "chrome.exe",
            Browser::Brave => "brave.exe",
            Browser::Vivaldi => "vivaldi.exe",
            Browser::Opera => "opera.exe",
            Browser::Firefox => "firefox.exe",
        }
    }

    /// Full path of the browser's program, from App Paths.
    pub fn exe_path(self) -> Option<PathBuf> {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        let sub = format!(
            r"Software\Microsoft\Windows\CurrentVersion\App Paths\{}",
            self.exe()
        );
        [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER]
            .into_iter()
            .find_map(|root| {
                let k = crate::registry::open_in(root, &sub, false)?;
                crate::registry::string(&k, None, "").map(|p| PathBuf::from(p.trim_matches('"')))
            })
    }

    /// The browser's own page for one extension (or its list).
    pub fn page(self, id: &str) -> String {
        match self {
            Browser::Edge => format!("edge://extensions/?id={id}"),
            Browser::Firefox => "about:addons".into(),
            Browser::Opera => "opera://extensions".into(),
            Browser::Brave => format!("brave://extensions/?id={id}"),
            Browser::Vivaldi => format!("vivaldi://extensions/?id={id}"),
            Browser::Chrome => format!("chrome://extensions/?id={id}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Extension {
    pub browser: Browser,
    pub id: String,
    pub name: String,
    pub version: String,
    /// Profile folder name ("Default", "Profile 1", …).
    pub profile: String,
}

const CHROMIUM: [(Browser, &str); 5] = [
    (Browser::Edge, r"Microsoft\Edge\User Data"),
    (Browser::Chrome, r"Google\Chrome\User Data"),
    (Browser::Brave, r"BraveSoftware\Brave-Browser\User Data"),
    (Browser::Vivaldi, r"Vivaldi\User Data"),
    (Browser::Opera, r"Opera Software\Opera Stable"),
];

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(fs::read_to_string(p).ok()?.trim_start_matches('\u{feff}')).ok()
}

/// `__MSG_appName__` → the text from `_locales/<locale>/messages.json`.
fn localize(name: &str, ext_dir: &Path, default_locale: Option<&str>) -> String {
    let Some(key) = name
        .strip_prefix("__MSG_")
        .and_then(|k| k.strip_suffix("__"))
    else {
        return name.to_string();
    };
    let locales = [default_locale.unwrap_or("en"), "en", "en_US"];
    for loc in locales {
        let Some(msgs) = read_json(&ext_dir.join("_locales").join(loc).join("messages.json"))
        else {
            continue;
        };
        // message keys are case-insensitive
        if let Some(obj) = msgs.as_object() {
            for (k, v) in obj {
                if k.eq_ignore_ascii_case(key)
                    && let Some(m) = v.get("message").and_then(Value::as_str)
                {
                    return m.to_string();
                }
            }
        }
    }
    name.to_string()
}

/// One profile's `Extensions\<id>\<version>\manifest.json` files.
fn chromium_profile(browser: Browser, profile: &Path, out: &mut Vec<Extension>) {
    let pname = profile
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for e in fs::read_dir(profile.join("Extensions"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let id = e.file_name().to_string_lossy().into_owned();
        // newest version folder
        let Some(ver_dir) = fs::read_dir(e.path())
            .into_iter()
            .flatten()
            .flatten()
            .map(|v| v.path())
            .filter(|p| p.join("manifest.json").is_file())
            .max()
        else {
            continue;
        };
        let Some(m) = read_json(&ver_dir.join("manifest.json")) else {
            continue;
        };
        // themes and hidden component apps aren't extensions people manage
        if m.get("theme").is_some() || m.get("app").is_some() {
            continue;
        }
        let raw = m.get("name").and_then(Value::as_str).unwrap_or(&id);
        let name = localize(
            raw,
            &ver_dir,
            m.get("default_locale").and_then(Value::as_str),
        );
        out.push(Extension {
            browser,
            id,
            name,
            version: m
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            profile: pname.clone(),
        });
    }
}

fn firefox(appdata: &Path, out: &mut Vec<Extension>) {
    for p in fs::read_dir(appdata.join(r"Mozilla\Firefox\Profiles"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let Some(j) = read_json(&p.path().join("extensions.json")) else {
            continue;
        };
        let pname = p.file_name().to_string_lossy().into_owned();
        for a in j
            .get("addons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            // only extensions the user installed (not Mozilla's built-ins)
            if s("type") != "extension"
                || s("location") != "app-profile"
                || s("id").ends_with("@mozilla.org")
            {
                continue;
            }
            let name = a
                .get("defaultLocale")
                .and_then(|l| l.get("name"))
                .and_then(Value::as_str)
                .map_or_else(|| s("id"), str::to_string);
            out.push(Extension {
                browser: Browser::Firefox,
                id: s("id"),
                name,
                version: s("version"),
                profile: pname.clone(),
            });
        }
    }
}

/// Every installed extension, grouped by browser then sorted by name.
pub fn list() -> Vec<Extension> {
    let mut out = Vec::new();
    let la = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    if let Some(la) = &la {
        for (b, dir) in CHROMIUM {
            let data = la.join(dir);
            let mut profiles: Vec<PathBuf> = fs::read_dir(&data)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join("Extensions").is_dir())
                .collect();
            if data.join("Extensions").is_dir() {
                profiles.push(data.clone()); // Opera keeps them at the top
            }
            for p in profiles {
                chromium_profile(b, &p, &mut out);
            }
        }
    }
    if let Some(ad) = std::env::var_os("APPDATA").map(PathBuf::from) {
        firefox(&ad, &mut out);
    }
    out.sort_by_key(|e| (e.browser as u8, e.name.to_lowercase()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_chromium_profile_with_localized_names() {
        let root = std::env::temp_dir().join(format!("bosun-ext-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let v = root.join(r"Default\Extensions\abcd\1.2.0_0");
        fs::create_dir_all(v.join(r"_locales\en")).unwrap();
        fs::write(
            v.join("manifest.json"),
            r#"{"name":"__MSG_appName__","version":"1.2.0","default_locale":"en"}"#,
        )
        .unwrap();
        fs::write(
            v.join(r"_locales\en\messages.json"),
            r#"{"APPNAME":{"message":"Color Picker"}}"#,
        )
        .unwrap();
        let theme = root.join(r"Default\Extensions\them\1_0");
        fs::create_dir_all(&theme).unwrap();
        fs::write(
            theme.join("manifest.json"),
            r#"{"name":"Dark","version":"1","theme":{}}"#,
        )
        .unwrap();
        let mut out = Vec::new();
        chromium_profile(Browser::Edge, &root.join("Default"), &mut out);
        assert_eq!(out.len(), 1, "themes are skipped");
        assert_eq!(
            (out[0].name.as_str(), out[0].version.as_str()),
            ("Color Picker", "1.2.0")
        );
        assert_eq!(out[0].profile, "Default");
        assert_eq!(Browser::Edge.page("abcd"), "edge://extensions/?id=abcd");
        fs::remove_dir_all(root).unwrap();
    }

    /// Read-only: `cargo test -p bosun-core real_extensions -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_extensions() {
        let all = list();
        for e in &all {
            println!(
                "{:<16} {} {} [{}]",
                e.browser.label(),
                e.name,
                e.version,
                e.profile
            );
        }
        println!("{} extensions", all.len());
    }
}
