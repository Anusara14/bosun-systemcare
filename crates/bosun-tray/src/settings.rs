//! Tiny `key=value` settings file at `%APPDATA%\Bosun\tray.ini`.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub widget_visible: bool,
    pub widget_pos: Option<(i32, i32)>,
    pub auto_ram_clean: bool,
    /// The widget stays above other windows ("Pin it up front").
    pub widget_pinned: bool,
    /// The widget is drawn see-through.
    pub widget_transparent: bool,
    /// Auto Clean: junk files once a day, and when it last ran (unix secs).
    pub auto_clean: bool,
    pub auto_clean_last: u64,
    /// Turn Turbo Boost on when the tray starts (at Windows startup).
    pub turbo_at_startup: bool,
    /// Notifications (Settings > Notifications).
    pub notify_auto_ram: bool,
    pub notify_homepage: bool,
    pub notify_auto_clean: bool,
    /// When the tray last looked for a newer Bosun (unix secs).
    pub update_last: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            widget_visible: true,
            widget_pos: None,
            auto_ram_clean: false,
            widget_pinned: true,
            widget_transparent: false,
            auto_clean: false,
            auto_clean_last: 0,
            turbo_at_startup: false,
            notify_auto_ram: true,
            notify_homepage: true,
            notify_auto_clean: true,
            update_last: 0,
        }
    }
}

/// The main window's skin choice (`app.ini`, written by bosun.exe): the
/// widget, monitor and menus follow it.
pub fn light_skin() -> bool {
    std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("Bosun").join("app.ini"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|t| {
            t.lines()
                .filter_map(|l| l.split_once('='))
                .any(|(k, v)| k.trim() == "light_skin" && v.trim() == "1")
        })
}

fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Bosun").join("tray.ini"))
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        let (mut x, mut y) = (None, None);
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "widget_visible" => s.widget_visible = v.trim() != "0",
                "auto_ram_clean" => s.auto_ram_clean = v.trim() == "1",
                "widget_pinned" => s.widget_pinned = v.trim() != "0",
                "widget_transparent" => s.widget_transparent = v.trim() == "1",
                "auto_clean" => s.auto_clean = v.trim() == "1",
                "auto_clean_last" => s.auto_clean_last = v.trim().parse().unwrap_or(0),
                "turbo_at_startup" => s.turbo_at_startup = v.trim() == "1",
                "notify_auto_ram" => s.notify_auto_ram = v.trim() != "0",
                "notify_homepage" => s.notify_homepage = v.trim() != "0",
                "notify_auto_clean" => s.notify_auto_clean = v.trim() != "0",
                "update_last" => s.update_last = v.trim().parse().unwrap_or(0),
                "widget_x" => x = v.trim().parse().ok(),
                "widget_y" => y = v.trim().parse().ok(),
                _ => {}
            }
        }
        s.widget_pos = x.zip(y);
        s
    }

    pub fn serialize(&self) -> String {
        let mut out = format!(
            "widget_visible={}\nauto_ram_clean={}\nwidget_pinned={}\nwidget_transparent={}\n",
            u8::from(self.widget_visible),
            u8::from(self.auto_ram_clean),
            u8::from(self.widget_pinned),
            u8::from(self.widget_transparent)
        );
        out.push_str(&format!(
            "auto_clean={}\nauto_clean_last={}\nturbo_at_startup={}\n",
            u8::from(self.auto_clean),
            self.auto_clean_last,
            u8::from(self.turbo_at_startup)
        ));
        out.push_str(&format!(
            "notify_auto_ram={}
notify_homepage={}
notify_auto_clean={}
",
            u8::from(self.notify_auto_ram),
            u8::from(self.notify_homepage),
            u8::from(self.notify_auto_clean)
        ));
        out.push_str(&format!("update_last={}\n", self.update_last));
        if let Some((x, y)) = self.widget_pos {
            out.push_str(&format!("widget_x={x}\nwidget_y={y}\n"));
        }
        out
    }

    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map_or_else(Self::default, |t| Self::parse(&t))
    }

    /// Best effort: losing a widget position is not worth bothering the user about.
    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, self.serialize());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let s = Settings {
            widget_visible: false,
            widget_pos: Some((-120, 40)),
            auto_ram_clean: true,
            widget_pinned: false,
            widget_transparent: true,
            auto_clean: true,
            auto_clean_last: 1_795_000_000,
            turbo_at_startup: true,
            notify_auto_ram: false,
            notify_homepage: true,
            notify_auto_clean: false,
            update_last: 1_795_000_100,
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
    }

    #[test]
    fn tolerates_garbage() {
        let s = Settings::parse("nonsense\nwidget_x=abc\nwidget_y=5\n");
        assert_eq!(s, Settings::default());
    }
}
