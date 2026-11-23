//! Title bar, tab strip with ring gauges, and the table frame.

use super::*;

pub(super) fn draw_title(p: &Painter, m: &Monitor) {
    let f = &p.g.f;
    // Logo sits on the content's left edge (x = 10, same as the table frame).
    p.logo(FRAME_L, TITLE_CY - 8.5, 17.0);
    let title = rect(FRAME_L + 25.0, 0.0, 300.0, 2.0 * TITLE_CY);
    p.text(
        "Performance Monitor",
        &f.m13,
        title,
        col(mc::TEXT),
        Align::Left,
    );
    let (bx, by, br) = BACK;
    p.circle(
        bx,
        by,
        br,
        col(if m.hover == Hit::Back {
            0x444444
        } else {
            mc::BTN
        }),
    );
    let back = rect(bx - br, by - br, 2.0 * br, 2.0 * br);
    p.glyph(glyph::BACK, &f.icon_sm, back, col(mc::TEXT_2));
    let (cx, cy) = CLOSE;
    let c = col(if m.hover == Hit::Close {
        mc::TEXT
    } else {
        mc::TEXT_2
    });
    p.line(cx - 4.5, cy - 4.5, cx + 4.5, cy + 4.5, 1.8, c);
    p.line(cx + 4.5, cy - 4.5, cx - 4.5, cy + 4.5, 1.8, c);
}

/// Table frame + header row; the selected tab opens into the header like a folder tab.
pub(super) fn draw_frame(p: &Painter, m: &Monitor) {
    let line = col(mc::LINE);
    let width = FRAME_R - FRAME_L + 1.0;
    p.fill(
        rect(FRAME_L, FRAME_TOP, width, HEAD_BOTTOM - FRAME_TOP),
        col(mc::PANEL),
    );
    p.fill(
        rect(FRAME_L, LIST_TOP, width, LIST_BOTTOM - LIST_TOP),
        col(mc::LIST),
    );
    let sel = TABS.iter().position(|t| *t == m.tab).unwrap_or(0);
    let tr = tab_rect(sel);
    p.line(
        FRAME_L,
        FRAME_TOP + 0.5,
        tr.left,
        FRAME_TOP + 0.5,
        1.0,
        line,
    );
    p.line(
        tr.right,
        FRAME_TOP + 0.5,
        FRAME_R + 1.0,
        FRAME_TOP + 0.5,
        1.0,
        line,
    );
    p.line(
        FRAME_L,
        HEAD_BOTTOM + 0.5,
        FRAME_R + 1.0,
        HEAD_BOTTOM + 0.5,
        1.0,
        line,
    );
    p.line(
        FRAME_L,
        LIST_BOTTOM + 0.5,
        FRAME_R + 1.0,
        LIST_BOTTOM + 0.5,
        1.0,
        line,
    );
    p.line(
        FRAME_L + 0.5,
        FRAME_TOP,
        FRAME_L + 0.5,
        LIST_BOTTOM + 1.0,
        1.0,
        line,
    );
    p.line(
        FRAME_R + 0.5,
        FRAME_TOP,
        FRAME_R + 0.5,
        LIST_BOTTOM + 1.0,
        1.0,
        line,
    );
}

pub(super) fn draw_tab_chrome(p: &Painter, r: D2D_RECT_F) {
    let line = col(mc::LINE);
    p.fill(
        rect(r.left, r.top, TAB_W, r.bottom - r.top + 1.0),
        col(mc::PANEL),
    );
    p.line(
        r.left + 0.5,
        r.top + 3.0,
        r.left + 0.5,
        r.bottom + 1.0,
        1.0,
        line,
    );
    p.line(
        r.right - 0.5,
        r.top + 3.0,
        r.right - 0.5,
        r.bottom + 1.0,
        1.0,
        line,
    );
    p.fill_rr(rect(r.left, r.top, TAB_W, 4.0), 2.0, col(mc::ACCENT));
    p.fill(rect(r.left, r.top + 2.0, TAB_W, 2.0), col(mc::ACCENT));
}

pub(super) fn draw_tabs(p: &Painter, m: &Monitor, d: &Data) {
    let v = &d.vitals;
    let f = &p.g.f;
    for (i, t) in TABS.iter().enumerate() {
        let r = tab_rect(i);
        let selected = m.tab == *t;
        if selected {
            draw_tab_chrome(p, r);
        } else {
            hover_fill(p, m.hover == Hit::Tab(*t), r);
        }
        let cx = (r.left + r.right) / 2.0;
        let pct = match t {
            Tab::Ram => Some(v.mem.load_pct as f32),
            Tab::Cpu => Some(v.cpu_pct),
            Tab::Disk => Some(v.disk_pct),
            _ => None,
        };
        let inner = rect(cx - RING_R, RING_CY - RING_R, 2.0 * RING_R, 2.0 * RING_R);
        match pct {
            Some(pc) => {
                p.ring(cx, RING_CY, RING_R, RING_W, col(mc::TRACK));
                p.arc(cx, RING_CY, RING_R, RING_W, pc / 100.0, col(mc::RING));
                p.text(
                    &format!("{pc:.0}%"),
                    &f.m13,
                    inner,
                    col(mc::ACCENT),
                    Align::Center,
                );
            }
            None => {
                p.ring(cx, RING_CY, RING_R, RING_W, col(mc::RING));
                if *t == Tab::Metrics {
                    draw_bars(p, cx, RING_CY);
                } else {
                    p.glyph(glyph::WIFI, &f.icon20, inner, col(mc::RING));
                }
            }
        }
        let lc = if selected { mc::TEXT } else { mc::TEXT_2 };
        let lr = rect(r.left, LABEL_CY - 10.0, TAB_W, 20.0);
        p.text(t.label(), &f.m12, lr, col(lc), Align::Center);
    }
}

/// Metrics tab icon: three rising bars on a baseline.
pub(super) fn draw_bars(p: &Painter, cx: f32, cy: f32) {
    let c = col(mc::RING);
    for (i, h) in [5.0f32, 9.0, 13.0].into_iter().enumerate() {
        let x = cx - 5.0 + i as f32 * 5.0;
        p.line(x, cy + 4.5, x, cy + 4.5 - h, 2.2, c);
    }
    p.line(cx - 8.0, cy + 7.5, cx + 8.0, cy + 7.5, 1.6, c);
}
