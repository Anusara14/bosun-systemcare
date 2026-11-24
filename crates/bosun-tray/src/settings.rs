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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            widget_visible: true,
            widget_pos: None,
            auto_ram_clean: false,
            widget_pinned: true,
            widget_transparent: false,
        }
    }
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
        };
        assert_eq!(Settings::parse(&s.serialize()), s);
    }

    #[test]
    fn tolerates_garbage() {
        let s = Settings::parse("nonsense\nwidget_x=abc\nwidget_y=5\n");
        assert_eq!(s, Settings::default());
    }
}
