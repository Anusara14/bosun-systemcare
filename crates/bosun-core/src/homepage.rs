//! Homepage Advisor (spec §2.3): the default browser and, per Chromium
//! profile, its homepage, startup pages and search engine, so a change made
//! behind the user's back (adware, bundled installers) can be flagged.
//! Read-only: we warn and point to the browser's settings; we don't rewrite
//! browser preferences.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use windows::Win32::System::Registry::HKEY_CURRENT_USER;

use crate::extensions::Browser;

/// The browser Windows opens links with, from the user's http choice.
pub fn default_browser() -> Option<Browser> {
    let k = crate::registry::open_in(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\http\UserChoice",
        false,
    )?;
    let prog = crate::registry::string(&k, None, "ProgId")?.to_ascii_lowercase();
    Some(if prog.starts_with("msedge") {
        Browser::Edge
    } else if prog.starts_with("chrome") {
        Browser::Chrome
    } else if prog.starts_with("brave") {
        Browser::Brave
    } else if prog.starts_with("vivaldi") {
        Browser::Vivaldi
    } else if prog.starts_with("opera") {
        Browser::Opera
    } else if prog.starts_with("firefox") {
        Browser::Firefox
    } else {
        return None;
    })
}

/// What a browser profile opens and searches with.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Settings {
    pub homepage: String,
    pub startup: Vec<String>,
    pub search: String,
}

/// Reads one Chromium profile's `Preferences`.
pub fn parse(prefs: &str) -> Settings {
    let Ok(v) = serde_json::from_str::<Value>(prefs.trim_start_matches('\u{feff}')) else {
        return Settings::default();
    };
    let s = |p: &str| {
        v.pointer(p)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Settings {
        homepage: s("/homepage"),
        startup: v
            .pointer("/session/startup_urls")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
        search: s("/default_search_provider_data/template_url_data/short_name"),
    }
}

const DATA: [(Browser, &str); 3] = [
    (
        Browser::Edge,
        r"Microsoft\Edge\User Data\Default\Preferences",
    ),
    (
        Browser::Chrome,
        r"Google\Chrome\User Data\Default\Preferences",
    ),
    (
        Browser::Brave,
        r"BraveSoftware\Brave-Browser\User Data\Default\Preferences",
    ),
];

/// Settings of each installed Chromium browser's default profile.
pub fn current() -> Vec<(Browser, Settings)> {
    let Some(la) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return Vec::new();
    };
    DATA.iter()
        .filter_map(|(b, rel)| {
            fs::read_to_string(la.join(rel))
                .ok()
                .map(|t| (*b, parse(&t)))
        })
        .collect()
}

/// One line describing the default browser's settings now.
pub fn fingerprint() -> String {
    let b = default_browser();
    let s = b
        .and_then(|b| current().into_iter().find(|(x, _)| *x == b))
        .map(|(_, s)| s)
        .unwrap_or_default();
    format!(
        "{}|{}|{}|{}",
        b.map_or("", |b| b.label()),
        s.homepage,
        s.startup.join(" "),
        s.search
    )
}

fn baseline_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(r"Bosun\homepage.txt"))
}

/// The Advisor is watching (a baseline was saved).
pub fn advisor_on() -> bool {
    baseline_path().is_some_and(|p| p.is_file())
}

/// Turns the Advisor on (saves today's settings as the baseline) or off.
pub fn set_advisor(on: bool) -> std::io::Result<()> {
    let p = baseline_path().ok_or_else(|| std::io::Error::other("APPDATA is unset"))?;
    if on {
        if let Some(d) = p.parent() {
            fs::create_dir_all(d)?;
        }
        fs::write(p, fingerprint())
    } else if p.is_file() {
        fs::remove_file(p)
    } else {
        Ok(())
    }
}

/// While watching: the browser, homepage, startup pages or search engine
/// differ from the baseline.
pub fn changed() -> bool {
    baseline_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .is_some_and(|b| b.trim() != fingerprint())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_homepage_startup_and_search() {
        let p = r#"{"homepage":"https://example.org","session":{"startup_urls":["https://a.test","https://b.test"]},
                   "default_search_provider_data":{"template_url_data":{"short_name":"Bing"}}}"#;
        let s = parse(p);
        assert_eq!(s.homepage, "https://example.org");
        assert_eq!(s.startup, ["https://a.test", "https://b.test"]);
        assert_eq!(s.search, "Bing");
        assert_eq!(parse("not json"), Settings::default());
    }
}
