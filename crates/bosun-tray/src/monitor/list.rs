//! Process table: the list model (sections + rows) and how it is drawn,
//! plus the Metrics and Network tab contents.

use super::*;

// ---------- list model (pure) ----------

#[derive(Debug)]
pub(super) enum Line<'a> {
    Section {
        idx: usize,
        title: &'static str,
        total: String,
        collapsed: bool,
    },
    Row(&'a Group),
}

pub(super) fn metric(g: &Group, tab: Tab) -> f64 {
    match tab {
        Tab::Cpu => f64::from(g.cpu_pct),
        Tab::Disk => g.io_bps as f64,
        _ => g.private_bytes as f64,
    }
}

pub(super) fn value(g: &Group, tab: Tab) -> String {
    match tab {
        Tab::Cpu => format!("{:.1}%", g.cpu_pct),
        Tab::Disk => format::rate(g.io_bps),
        _ => format::bytes(g.private_bytes),
    }
}

pub(super) fn total(rows: &[&Group], tab: Tab) -> String {
    match tab {
        Tab::Cpu => format!("{:.1}%", rows.iter().map(|g| g.cpu_pct).sum::<f32>()),
        Tab::Disk => format::rate(rows.iter().map(|g| g.io_bps).sum()),
        _ => format::bytes(rows.iter().map(|g| g.private_bytes).sum()),
    }
}

pub(super) fn section_titles(tab: Tab) -> [&'static str; 2] {
    match tab {
        Tab::Cpu => ["Processes with high CPU usage", "Background processes"],
        Tab::Disk => ["Processes with high Disk I/O", "Background processes"],
        _ => ["Processes with high RAM usage", "Background processes"],
    }
}

/// Section rows + process rows, sorted by the active tab's metric.
pub(super) fn lines(groups: &[Group], tab: Tab, collapsed: [bool; 2]) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    for (idx, title) in section_titles(tab).into_iter().enumerate() {
        let mut rows: Vec<&Group> = groups
            .iter()
            .filter(|g| g.has_window == (idx == 0))
            .collect();
        if rows.is_empty() {
            continue;
        }
        rows.sort_by(|a, b| metric(b, tab).total_cmp(&metric(a, tab)));
        out.push(Line::Section {
            idx,
            title,
            total: total(&rows, tab),
            collapsed: collapsed[idx],
        });
        if !collapsed[idx] {
            out.extend(rows.into_iter().map(Line::Row));
        }
    }
    out
}

pub(super) fn header(p: &Painter, left: &str, right: &str) {
    let f = &p.g.f;
    let y = FRAME_TOP + 1.0;
    let h = HEAD_BOTTOM - y;
    p.text(
        left,
        &f.m12,
        rect(COL_NAME_X, y, 200.0, h),
        col(mc::TEXT_3),
        Align::Left,
    );
    p.text(
        right,
        &f.m12,
        rect(COL_VALUE_X, y, 160.0, h),
        col(mc::TEXT_3),
        Align::Left,
    );
    for x in [SEP1_X, SEP2_X] {
        p.line(x + 0.5, y + 7.0, x + 0.5, y + h - 7.0, 1.0, col(0x4A4A4A));
    }
}

pub(super) fn draw_processes(p: &Painter, m: &mut Monitor, groups: &[Group]) {
    let col_name = match m.tab {
        Tab::Cpu => "CPU",
        Tab::Disk => "Disk I/O",
        _ => "RAM",
    };
    header(p, "Processes", col_name);
    let ls = lines(groups, m.tab, m.collapsed);
    let view = LIST_BOTTOM - LIST_TOP;
    m.content_h = ls.len() as f32 * ROW_H;
    m.scroll = m.scroll.clamp(0.0, (m.content_h - view).max(0.0));
    p.clip(rect(FRAME_L + 1.0, LIST_TOP, FRAME_R - FRAME_L - 1.0, view));
    for (i, l) in ls.iter().enumerate() {
        let y = LIST_TOP + i as f32 * ROW_H - m.scroll;
        if y + ROW_H < LIST_TOP || y > LIST_BOTTOM {
            continue;
        }
        match l {
            Line::Section {
                idx,
                title,
                total,
                collapsed,
            } => draw_section(p, m, *idx, y, (title, total), *collapsed),
            Line::Row(g) => draw_row(p, m, g, y),
        }
    }
    p.unclip();
    draw_scrollbar(p, m.scroll, m.content_h, view);
    if groups.is_empty() {
        let r = rect(0.0, LIST_TOP, W, 60.0);
        p.text(
            "Reading processes…",
            &p.g.f.m12,
            r,
            col(mc::TEXT_3),
            Align::Center,
        );
    }
}

pub(super) fn draw_scrollbar(p: &Painter, scroll: f32, content: f32, view: f32) {
    if content <= view {
        return;
    }
    let track = view - 4.0;
    let h = (view / content * track).max(24.0);
    let y = LIST_TOP + 2.0 + (scroll / (content - view)) * (track - h);
    p.fill_rr(rect(501.0, y, 6.0, h), 3.0, col(mc::SCROLL));
}

pub(super) fn row_band(y: f32) -> D2D_RECT_F {
    rect(FRAME_L + 1.0, y, FRAME_R - FRAME_L - 9.0, ROW_H)
}

pub(super) fn draw_section(
    p: &Painter,
    m: &Monitor,
    idx: usize,
    y: f32,
    (title, total): (&str, &str),
    collapsed: bool,
) {
    let f = &p.g.f;
    hover_fill(p, m.hover == Hit::Section(idx), row_band(y));
    let tw = p.g.measure(title, &f.m12);
    p.text(
        title,
        &f.m12,
        rect(COL_NAME_X, y, tw + 2.0, ROW_H),
        col(mc::TEXT),
        Align::Left,
    );
    let tot = format!("total {total}");
    let x2 = COL_NAME_X + tw + 19.0;
    let ttw = p.g.measure(&tot, &f.m12);
    p.text(
        &tot,
        &f.m12,
        rect(x2, y, ttw + 2.0, ROW_H),
        col(mc::TEXT),
        Align::Left,
    );
    let rule_x = (x2 + ttw + 32.0).max(285.0);
    p.line(rule_x, y + 15.5, 457.0, y + 15.5, 1.0, col(0x3A3A3A));
    if collapsed {
        p.glyph(
            glyph::CHEVRON_RIGHT,
            &f.icon_sm,
            rect(470.0, y, 18.0, ROW_H),
            col(mc::TEXT),
        );
    } else {
        p.caret_down(479.0, y + 15.0, 9.0, col(mc::TEXT));
    }
}

pub(super) fn row_icon(m: &mut Monitor, g: &Group) -> Option<ID2D1Bitmap> {
    let path = g.path.as_ref()?;
    let rt = m.rt.as_ref().map(|(rt, _)| rt.clone())?;
    m.icons
        .entry(path.clone())
        .or_insert_with(|| {
            file_icon(path).and_then(|h| {
                let b = graphics().icon_bitmap(&rt, h);
                // SAFETY: SHGetFileInfo gave us ownership of the icon.
                let _ = unsafe { DestroyIcon(h) };
                b
            })
        })
        .clone()
}

pub(super) fn draw_row(p: &Painter, m: &mut Monitor, g: &Group, y: f32) {
    let f = &p.g.f;
    let checked = m.checked.contains(&g.key);
    hover_fill(p, m.hover == Hit::Row(g.key.clone()), row_band(y));
    let cb = rect(21.5, y + 7.5, 15.0, 15.0);
    if g.protected {
        p.stroke_rr(cb, 2.0, 1.0, cola(mc::CHECK_BORDER, 0.35));
    } else if checked {
        p.fill_rr(cb, 2.0, col(mc::ACCENT));
        p.glyph(glyph::CHECK, &f.icon_sm, cb, col(0x000000));
    } else {
        p.stroke_rr(cb, 2.0, 1.0, col(mc::CHECK_BORDER));
    }
    let ir = rect(45.0, y + 7.0, 16.0, 16.0);
    match row_icon(m, g) {
        Some(b) => p.bitmap(&b, ir, 1.0),
        None => {
            p.fill_rr(ir, 3.0, col(0x2E2E2E));
            let initial = g
                .name
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            p.text(&initial, &f.caption, ir, col(mc::TEXT_2), Align::Center);
        }
    }
    let count = if g.pids.len() > 1 {
        format!(" ({})", g.pids.len())
    } else {
        String::new()
    };
    let name = format!("{}{count}", g.name);
    let nr = rect(66.0, y, COL_VALUE_X - 76.0, ROW_H);
    p.text(&name, &f.m12, nr, col(mc::TEXT), Align::Left);
    let vr = rect(COL_VALUE_X, y, 140.0, ROW_H);
    p.text(&value(g, m.tab), &f.m12, vr, col(mc::TEXT), Align::Left);
}

pub(super) fn kv_rows(p: &Painter, rows: &[(&str, String)]) {
    let f = &p.g.f;
    for (i, (k, v)) in rows.iter().enumerate() {
        let y = LIST_TOP + i as f32 * ROW_H;
        p.text(
            k,
            &f.m12,
            rect(COL_NAME_X, y, 240.0, ROW_H),
            col(mc::TEXT_2),
            Align::Left,
        );
        p.text(
            v,
            &f.m12,
            rect(COL_VALUE_X, y, 230.0, ROW_H),
            col(mc::TEXT),
            Align::Left,
        );
    }
}

pub(super) fn draw_metrics(p: &Painter, d: &Data) {
    header(p, "Metric", "Value");
    let v = &d.vitals;
    let procs: usize = d.groups.iter().map(|g| g.pids.len()).sum();
    let apps = d.groups.iter().filter(|g| g.has_window).count();
    // SAFETY: trivial system call.
    let up = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() } / 1000;
    let mem = format!(
        "{} of {}",
        format::bytes(v.mem.used()),
        format::bytes(v.mem.total)
    );
    let net = format!(
        "↓ {}   ↑ {}",
        format::rate(v.rx_bps),
        format::rate(v.tx_bps)
    );
    kv_rows(
        p,
        &[
            ("CPU usage", format!("{:.0}%", v.cpu_pct)),
            ("Memory in use", mem),
            ("Disk active time", format!("{:.0}%", v.disk_pct)),
            ("Network", net),
            ("Processes / apps", format!("{procs} / {apps}")),
            (
                "Up time",
                format!(
                    "{}d {:02}:{:02}",
                    up / 86_400,
                    (up / 3600) % 24,
                    (up / 60) % 60
                ),
            ),
        ],
    );
}

pub(super) fn draw_network(p: &Painter, d: &Data) {
    header(p, "Adapter", "Download / Upload");
    if d.adapters.is_empty() {
        let r = rect(0.0, LIST_TOP, W, 60.0);
        p.text(
            "No connected network adapters",
            &p.g.f.m12,
            r,
            col(mc::TEXT_3),
            Align::Center,
        );
        return;
    }
    let rows: Vec<(&str, String)> = d
        .adapters
        .iter()
        .map(|a| {
            let rates = format!(
                "↓ {}   ↑ {}",
                format::rate(a.rx_bps),
                format::rate(a.tx_bps)
            );
            (a.name.as_str(), rates)
        })
        .collect();
    kv_rows(p, &rows);
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn g(key: &str, mem: u64, cpu: f32, win: bool) -> Group {
        Group {
            key: key.into(),
            name: key.into(),
            path: None,
            pids: vec![1],
            private_bytes: mem,
            cpu_pct: cpu,
            io_bps: 0,
            has_window: win,
            protected: false,
        }
    }

    #[test]
    fn lines_split_apps_and_background_and_sort_per_tab() {
        let gs = vec![
            g("a", 10, 9.0, true),
            g("b", 30, 1.0, true),
            g("c", 5, 0.0, false),
        ];
        let ram = lines(&gs, Tab::Ram, [false, false]);
        assert!(matches!(ram[0], Line::Section { idx: 0, title, .. } if title.contains("RAM")));
        assert!(matches!(ram[1], Line::Row(r) if r.key == "b"));
        assert!(matches!(ram[3], Line::Section { idx: 1, .. }));
        let cpu = lines(&gs, Tab::Cpu, [false, false]);
        assert!(matches!(cpu[1], Line::Row(r) if r.key == "a"));
    }

    #[test]
    fn collapsed_sections_hide_rows() {
        let gs = vec![g("a", 10, 0.0, true), g("c", 5, 0.0, false)];
        assert_eq!(lines(&gs, Tab::Ram, [true, false]).len(), 3);
    }

    #[test]
    fn totals_format_by_tab() {
        let gs = [
            g("a", 1024 * 1024, 1.5, true),
            g("b", 1024 * 1024, 2.0, true),
        ];
        let refs: Vec<&Group> = gs.iter().collect();
        assert_eq!(total(&refs, Tab::Ram), "2.00 MB");
        assert_eq!(total(&refs, Tab::Cpu), "3.5%");
    }
}
