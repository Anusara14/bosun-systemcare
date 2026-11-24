//! Direct2D / DirectWrite helpers and the Bosun design tokens.

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_CAP_STYLE_ROUND, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_LINE_JOIN_ROUND, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1CreateFactory, ID2D1Bitmap,
    ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_LIGHT, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::core::{HSTRING, Result};
use windows_numerics::{Matrix3x2, Vector2};

/// Design tokens (SPEC.md §3.1).
pub mod tok {
    pub const TEXT: u32 = 0xFFFFFF;
    pub const RING: u32 = 0x1E9CFF;
}

/// Segoe Fluent Icons code points (fall back to Segoe MDL2 Assets on Windows 10).
pub mod glyph {
    pub const CHECK: char = '\u{E73E}';
    pub const CHEVRON_RIGHT: char = '\u{E76C}';
    pub const WIFI: char = '\u{E701}';
    pub const SEARCH: char = '\u{E721}';
    pub const SPEED: char = '\u{EC4A}';
    pub const CROP: char = '\u{E7A8}';
    pub const BACK: char = '\u{E72B}';
}

pub fn col(hex: u32) -> D2D1_COLOR_F {
    cola(hex, 1.0)
}

/// Blend of two colours (`t` = 0 → `a`, 1 → `b`), fully opaque.
pub fn mix(a: u32, b: u32, t: f32) -> D2D1_COLOR_F {
    let (ca, cb) = (col(a), col(b));
    D2D1_COLOR_F {
        r: ca.r + (cb.r - ca.r) * t,
        g: ca.g + (cb.g - ca.g) * t,
        b: ca.b + (cb.b - ca.b) * t,
        a: 1.0,
    }
}

pub fn cola(hex: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a,
    }
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

pub fn contains(r: &D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

pub struct Fonts {
    pub body: IDWriteTextFormat,
    pub body_sb: IDWriteTextFormat,
    pub caption: IDWriteTextFormat,
    pub small: IDWriteTextFormat,
    pub small_sb: IDWriteTextFormat,
    pub title: IDWriteTextFormat,
    pub icon: IDWriteTextFormat,
    pub icon_sm: IDWriteTextFormat,
    /// Widget / RAM-clean popup (classic Segoe UI, measured from the reference).
    pub w_text: IDWriteTextFormat,
    pub w_pct: IDWriteTextFormat,
    pub pop_label: IDWriteTextFormat,
    pub pop_value: IDWriteTextFormat,
    /// Performance Monitor (classic Segoe UI, reference sizes).
    pub m12: IDWriteTextFormat,
    pub m13: IDWriteTextFormat,
    pub icon20: IDWriteTextFormat,
    pub icon24: IDWriteTextFormat,
    pub icon12: IDWriteTextFormat,
    /// Main window: hero button label and page headings.
    pub hero: IDWriteTextFormat,
    pub h1: IDWriteTextFormat,
    pub m13sb: IDWriteTextFormat,
    pub m14sb: IDWriteTextFormat,
    /// Scan screen (reference cap heights: heading 15, title 15, % digits 50).
    pub s_head: IDWriteTextFormat,
    pub s_title: IDWriteTextFormat,
    pub s_pct: IDWriteTextFormat,
    pub s_sign: IDWriteTextFormat,
    pub s_size: IDWriteTextFormat,
}

pub struct Gfx {
    pub d2d: ID2D1Factory,
    pub dw: IDWriteFactory,
    /// Created on first use (only the monitor needs it).
    wic: std::cell::OnceCell<Option<IWICImagingFactory>>,
    pub round: ID2D1StrokeStyle,
    pub round_join: ID2D1StrokeStyle,
    pub f: Fonts,
}

impl Gfx {
    pub fn new() -> Result<Self> {
        // SAFETY: plain factory construction; COM is initialised by main.
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let round = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    ..Default::default()
                },
                None,
            )?;
            let round_join = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    ..Default::default()
                },
                None,
            )?;
            let ui = "Segoe UI Variable Text";
            // File check instead of enumerating the system font collection,
            // which costs megabytes of private memory.
            let icons = if fluent_icons_installed() {
                "Segoe Fluent Icons"
            } else {
                "Segoe MDL2 Assets"
            };
            let f = Fonts {
                body: text_format(&dw, ui, DWRITE_FONT_WEIGHT_NORMAL, 14.0)?,
                body_sb: text_format(&dw, ui, DWRITE_FONT_WEIGHT_SEMI_BOLD, 14.0)?,
                caption: text_format(&dw, ui, DWRITE_FONT_WEIGHT_NORMAL, 12.0)?,
                small: text_format(&dw, ui, DWRITE_FONT_WEIGHT_NORMAL, 13.0)?,
                small_sb: text_format(&dw, ui, DWRITE_FONT_WEIGHT_SEMI_BOLD, 13.0)?,
                title: text_format(&dw, ui, DWRITE_FONT_WEIGHT_SEMI_BOLD, 15.0)?,
                icon: text_format(&dw, icons, DWRITE_FONT_WEIGHT_NORMAL, 16.0)?,
                icon_sm: text_format(&dw, icons, DWRITE_FONT_WEIGHT_NORMAL, 10.0)?,
                w_text: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_NORMAL, 13.0)?,
                w_pct: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_NORMAL, 10.0)?,
                pop_label: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_NORMAL, 14.0)?,
                pop_value: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_NORMAL, 21.0)?,
                m12: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_NORMAL, 12.0)?,
                m13: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_NORMAL, 13.0)?,
                icon20: text_format(&dw, icons, DWRITE_FONT_WEIGHT_NORMAL, 20.0)?,
                icon24: text_format(&dw, icons, DWRITE_FONT_WEIGHT_NORMAL, 24.0)?,
                icon12: text_format(&dw, icons, DWRITE_FONT_WEIGHT_NORMAL, 12.0)?,
                hero: text_format(
                    &dw,
                    "Segoe UI",
                    windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_BOLD,
                    36.0,
                )?,
                m13sb: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_SEMI_BOLD, 13.0)?,
                m14sb: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_SEMI_BOLD, 14.0)?,
                h1: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_SEMI_BOLD, 20.0)?,
                s_head: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_SEMI_BOLD, 22.0)?,
                s_title: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_NORMAL, 21.0)?,
                s_pct: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_LIGHT, 72.0)?,
                s_sign: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_LIGHT, 20.0)?,
                s_size: text_format(&dw, "Segoe UI", DWRITE_FONT_WEIGHT_SEMI_BOLD, 17.0)?,
            };
            Ok(Self {
                d2d,
                dw,
                wic: std::cell::OnceCell::new(),
                round,
                round_join,
                f,
            })
        }
    }

    /// Advance width of `s` in DIPs.
    pub fn measure(&self, s: &str, f: &IDWriteTextFormat) -> f32 {
        let w: Vec<u16> = s.encode_utf16().collect();
        // SAFETY: temporary text layout, released on drop.
        unsafe {
            let Ok(l) = self.dw.CreateTextLayout(&w, f, 10_000.0, 100.0) else {
                return 0.0;
            };
            let mut m = Default::default();
            if l.GetMetrics(&mut m).is_err() {
                return 0.0;
            }
            m.widthIncludingTrailingWhitespace
        }
    }

    /// Converts an HICON into a bitmap owned by `rt`.
    pub fn icon_bitmap(&self, rt: &ID2D1RenderTarget, icon: HICON) -> Option<ID2D1Bitmap> {
        // SAFETY: WIC copies the icon pixels; the caller still owns `icon`.
        let wic = self
            .wic
            .get_or_init(|| unsafe {
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()
            })
            .as_ref()?;
        unsafe {
            let src = wic.CreateBitmapFromHICON(icon).ok()?;
            let conv = wic.CreateFormatConverter().ok()?;
            conv.Initialize(
                &src,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )
            .ok()?;
            rt.CreateBitmapFromWicBitmap(&conv, None).ok()
        }
    }
}

fn fluent_icons_installed() -> bool {
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
    std::path::Path::new(&windir)
        .join("Fonts")
        .join("SegoeIcons.ttf")
        .exists()
}

fn text_format(
    dw: &IDWriteFactory,
    family: &str,
    weight: DWRITE_FONT_WEIGHT,
    size: f32,
) -> Result<IDWriteTextFormat> {
    // SAFETY: plain DirectWrite object creation.
    unsafe {
        let f = dw.CreateTextFormat(
            &HSTRING::from(family),
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            &HSTRING::from("en-us"),
        )?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        let sign = dw.CreateEllipsisTrimmingSign(&f)?;
        let trim = DWRITE_TRIMMING {
            granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            ..Default::default()
        };
        f.SetTrimming(&trim, &sign)?;
        Ok(f)
    }
}

#[derive(Clone, Copy)]
pub enum Align {
    Left,
    Center,
}

impl Align {
    fn dw(self) -> DWRITE_TEXT_ALIGNMENT {
        match self {
            Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
            Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
        }
    }
}

/// Thin drawing wrapper: one brush, recoloured per call.
pub struct Painter<'a> {
    pub rt: &'a ID2D1RenderTarget,
    pub br: &'a ID2D1SolidColorBrush,
    pub g: &'a Gfx,
}

// SAFETY (whole impl): every call is a Direct2D draw on a render target that is
// between BeginDraw/EndDraw, with resources created from the same target/factory.
impl Painter<'_> {
    fn set(&self, c: D2D1_COLOR_F) {
        unsafe { self.br.SetColor(&c) };
    }

    pub fn clear(&self, c: D2D1_COLOR_F) {
        unsafe { self.rt.Clear(Some(&c)) };
    }

    pub fn fill(&self, r: D2D_RECT_F, c: D2D1_COLOR_F) {
        self.set(c);
        unsafe { self.rt.FillRectangle(&r, self.br) };
    }

    pub fn fill_rr(&self, r: D2D_RECT_F, radius: f32, c: D2D1_COLOR_F) {
        self.set(c);
        let rr = D2D1_ROUNDED_RECT {
            rect: r,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.rt.FillRoundedRectangle(&rr, self.br) };
    }

    pub fn stroke_rr(&self, r: D2D_RECT_F, radius: f32, w: f32, c: D2D1_COLOR_F) {
        self.set(c);
        let rr = D2D1_ROUNDED_RECT {
            rect: r,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.rt.DrawRoundedRectangle(&rr, self.br, w, None) };
    }

    pub fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: D2D1_COLOR_F) {
        self.set(c);
        unsafe {
            self.rt
                .DrawLine(Vector2::new(x0, y0), Vector2::new(x1, y1), self.br, w, None)
        };
    }

    pub fn circle(&self, cx: f32, cy: f32, r: f32, c: D2D1_COLOR_F) {
        self.set(c);
        let e = D2D1_ELLIPSE {
            point: Vector2::new(cx, cy),
            radiusX: r,
            radiusY: r,
        };
        unsafe { self.rt.FillEllipse(&e, self.br) };
    }

    pub fn ring(&self, cx: f32, cy: f32, r: f32, w: f32, c: D2D1_COLOR_F) {
        self.set(c);
        let e = D2D1_ELLIPSE {
            point: Vector2::new(cx, cy),
            radiusX: r,
            radiusY: r,
        };
        unsafe { self.rt.DrawEllipse(&e, self.br, w, None) };
    }

    /// Progress arc starting at 12 o'clock, `frac` in 0..=1, round caps.
    pub fn arc(&self, cx: f32, cy: f32, r: f32, w: f32, frac: f32, c: D2D1_COLOR_F) {
        self.arc_at(cx, cy, r, w, 0.0, frac, c);
    }

    /// Arc beginning at `start` (fraction of a turn clockwise from 12 o'clock).
    #[allow(clippy::too_many_arguments)]
    pub fn arc_at(&self, cx: f32, cy: f32, r: f32, w: f32, start: f32, frac: f32, c: D2D1_COLOR_F) {
        let frac = frac.clamp(0.0, 0.9999);
        if frac <= 0.001 {
            return;
        }
        let a0 = start * std::f32::consts::TAU;
        let a = a0 + frac * std::f32::consts::TAU;
        let start = Vector2::new(cx + r * a0.sin(), cy - r * a0.cos());
        let end = Vector2::new(cx + r * a.sin(), cy - r * a.cos());
        self.set(c);
        unsafe {
            let Ok(geo) = self.g.d2d.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = geo.Open() else { return };
            sink.BeginFigure(start, D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: end,
                size: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_F {
                    width: r,
                    height: r,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if frac > 0.5 {
                    D2D1_ARC_SIZE_LARGE
                } else {
                    D2D1_ARC_SIZE_SMALL
                },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
            self.rt.DrawGeometry(&geo, self.br, w, &self.g.round);
        }
    }

    pub fn text(&self, s: &str, f: &IDWriteTextFormat, r: D2D_RECT_F, c: D2D1_COLOR_F, a: Align) {
        self.set(c);
        let w: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            let _ = f.SetTextAlignment(a.dw());
            self.rt.DrawText(
                &w,
                f,
                &r,
                self.br,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    pub fn glyph(&self, g: char, f: &IDWriteTextFormat, r: D2D_RECT_F, c: D2D1_COLOR_F) {
        self.text(g.encode_utf8(&mut [0; 4]), f, r, c, Align::Center);
    }

    /// The Bosun tray mark (dark disc, blue ring, slanted white B), drawn as
    /// vectors so the always-on widget needs no image codecs. Geometry matches
    /// `brand/bosun-tray.svg` (260-unit design box).
    pub fn logo(&self, x: f32, y: f32, size: f32) {
        let s = size / 260.0;
        let (cx, cy) = (x + 130.0 * s, y + 130.0 * s);
        // Disc: lifted slightly at the top, like light falling on glass.
        let lift = (cx, cy - 60.0 * s, 200.0 * s);
        self.radial_at(
            (cx, cy, 122.0 * s),
            lift,
            &[(0.0, col(0x1B2532)), (1.0, col(0x05070A))],
        );
        // Ring: bright on top, deep blue underneath.
        let ring = [
            (0.0, col(0x8ED2FF)),
            (0.45, col(tok::RING)),
            (1.0, col(0x0B66C8)),
        ];
        self.ring_linear(cx, cy, 112.0 * s, 20.0 * s, &ring);
        // Gloss: a soft highlight across the upper half.
        let gloss = [(0.0, cola(0xFFFFFF, 0.22)), (1.0, cola(0xFFFFFF, 0.0))];
        self.ellipse_linear(cx, cy - 52.0 * s, 88.0 * s, 58.0 * s, &gloss);
        // Hairline rim: catches light at the top.
        let rim = [
            (0.0, cola(0xFFFFFF, 0.35)),
            (0.5, cola(0xFFFFFF, 0.0)),
            (1.0, cola(tok::RING, 0.35)),
        ];
        self.ring_linear(cx, cy, 121.0 * s, 2.0 * s, &rim);
        let v = Vector2::new;
        let arc = |to, r: f32| D2D1_ARC_SEGMENT {
            point: to,
            size: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_F {
                width: r,
                height: r,
            },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_SMALL,
        };
        // translate(136 128) scale(1.18) skewX(-9) translate(-141 -130), then place.
        let k = (-9.0f32).to_radians().tan();
        let (sc, tx, ty) = (1.18 * s, x + 136.0 * s, y + 128.0 * s);
        let m = Matrix3x2 {
            M11: sc,
            M12: 0.0,
            M21: k * sc,
            M22: sc,
            M31: tx + sc * (-141.0 + k * -130.0),
            M32: ty + sc * -130.0,
        };
        unsafe {
            let Ok(geo) = self.g.d2d.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = geo.Open() else { return };
            sink.BeginFigure(v(110.0, 178.0), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddLine(v(110.0, 82.0));
            sink.AddLine(v(144.0, 82.0));
            sink.AddArc(&arc(v(144.0, 128.0), 23.0));
            sink.AddLine(v(110.0, 128.0));
            sink.AddLine(v(150.0, 128.0));
            sink.AddArc(&arc(v(150.0, 178.0), 25.0));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
            self.rt.SetTransform(&m);
            // White B with a faint cool tint at its foot (brush in glyph space).
            let fill = [
                (0.0, col(0xFFFFFF)),
                (0.7, col(0xF4F9FF)),
                (1.0, col(0xD6ECFF)),
            ];
            match self.linear_brush((0.0, 82.0), (0.0, 178.0), &fill) {
                Some(b) => self.rt.DrawGeometry(&geo, &b, 24.0, &self.g.round_join),
                None => self
                    .rt
                    .DrawGeometry(&geo, self.br, 24.0, &self.g.round_join),
            }
            self.rt.SetTransform(&Matrix3x2::identity());
        }
    }

    /// Our rocket: bullet body, side fins, porthole and exhaust, centred on
    /// (cx, cy) in a ~20 DIP tall box. `flame` 0..1 lengthens the exhaust.
    pub fn rocket(&self, cx: f32, cy: f32, c: D2D1_COLOR_F, window: D2D1_COLOR_F, flame: f32) {
        let v = |x: f32, y: f32| Vector2::new(cx + x, cy + y);
        self.set(c);
        unsafe {
            let Ok(geo) = self.g.d2d.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = geo.Open() else { return };
            // body
            sink.BeginFigure(v(0.0, -9.5), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: v(2.6, -7.6),
                point2: v(3.4, -4.4),
                point3: v(3.4, 0.5),
            });
            sink.AddLine(v(3.4, 3.0));
            sink.AddLine(v(-3.4, 3.0));
            sink.AddLine(v(-3.4, 0.5));
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: v(-3.4, -4.4),
                point2: v(-2.6, -7.6),
                point3: v(0.0, -9.5),
            });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            // fins
            for side in [-1.0f32, 1.0] {
                sink.BeginFigure(v(3.4 * side, -0.8), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(v(5.6 * side, 3.4));
                sink.AddLine(v(5.6 * side, 5.0));
                sink.AddLine(v(3.4 * side, 3.4));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            }
            let _ = sink.Close();
            self.rt.FillGeometry(&geo, self.br, None);
        }
        self.circle(cx, cy - 4.0, 1.3, window);
        let len = 5.5 + 4.0 * flame;
        self.set(c);
        unsafe {
            self.rt
                .DrawLine(v(0.0, 3.5), v(0.0, 3.5 + len), self.br, 1.4, &self.g.round);
        }
    }

    /// Smooth concave joins where a circle (cx, cy, r) meets a horizontal bar
    /// spanning `top..bottom`, so the knob flows into the pill (fillet radius `rf`).
    #[allow(clippy::too_many_arguments)]
    pub fn fillets(
        &self,
        cx: f32,
        cy: f32,
        r: f32,
        top: f32,
        bottom: f32,
        rf: f32,
        c: D2D1_COLOR_F,
    ) {
        self.set(c);
        for (edge, dir) in [(top, -1.0f32), (bottom, 1.0)] {
            // Fillet circle: tangent to the bar edge and externally tangent to the knob.
            let fy = edge + dir * rf;
            let dy = fy - cy;
            let dx = ((r + rf).powi(2) - dy * dy).sqrt();
            if !dx.is_finite() {
                continue;
            }
            let fx = cx + dx;
            let k = r / (r + rf);
            let t = Vector2::new(cx + dx * k, cy + dy * k);
            unsafe {
                let Ok(geo) = self.g.d2d.CreatePathGeometry() else {
                    return;
                };
                let Ok(sink) = geo.Open() else { return };
                sink.BeginFigure(t, D2D1_FIGURE_BEGIN_FILLED);
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: Vector2::new(fx, edge),
                    size: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_F {
                        width: rf,
                        height: rf,
                    },
                    rotationAngle: 0.0,
                    sweepDirection: if dir < 0.0 {
                        windows::Win32::Graphics::Direct2D::D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
                    } else {
                        D2D1_SWEEP_DIRECTION_CLOCKWISE
                    },
                    arcSize: D2D1_ARC_SIZE_SMALL,
                });
                sink.AddLine(Vector2::new(cx, edge));
                sink.AddLine(Vector2::new(cx, cy));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                let _ = sink.Close();
                self.rt.FillGeometry(&geo, self.br, None);
            }
        }
    }

    /// Small filled caret (pointing down) centred on (cx, cy).
    pub fn caret_down(&self, cx: f32, cy: f32, w: f32, c: D2D1_COLOR_F) {
        self.set(c);
        let h = w * 0.55;
        unsafe {
            let Ok(geo) = self.g.d2d.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = geo.Open() else { return };
            sink.BeginFigure(
                Vector2::new(cx - w / 2.0, cy - h / 2.0),
                D2D1_FIGURE_BEGIN_FILLED,
            );
            sink.AddLine(Vector2::new(cx + w / 2.0, cy - h / 2.0));
            sink.AddLine(Vector2::new(cx, cy + h / 2.0));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
            self.rt.FillGeometry(&geo, self.br, None);
        }
    }

    /// Right-pointing arrow (shaft + chevron head), centred on (cx, cy).
    pub fn arrow(&self, cx: f32, cy: f32, size: f32, w: f32, c: D2D1_COLOR_F) {
        let h = size / 2.0;
        self.set(c);
        let v = |x: f32, y: f32| Vector2::new(cx + x, cy + y);
        unsafe {
            self.rt
                .DrawLine(v(-h, 0.0), v(h - 0.4, 0.0), self.br, w, &self.g.round);
            self.rt
                .DrawLine(v(h - 0.4, 0.0), v(0.3, -h + 0.6), self.br, w, &self.g.round);
            self.rt
                .DrawLine(v(h - 0.4, 0.0), v(0.3, h - 0.6), self.br, w, &self.g.round);
        }
    }

    fn linear_brush(
        &self,
        from: (f32, f32),
        to: (f32, f32),
        stops: &[(f32, D2D1_COLOR_F)],
    ) -> Option<windows::Win32::Graphics::Direct2D::ID2D1LinearGradientBrush> {
        let gs: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|&(position, color)| D2D1_GRADIENT_STOP { position, color })
            .collect();
        // SAFETY: temporary brush resources on the current render target.
        unsafe {
            let coll = self
                .rt
                .CreateGradientStopCollection(&gs, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                .ok()?;
            let props = windows::Win32::Graphics::Direct2D::D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2::new(from.0, from.1),
                endPoint: Vector2::new(to.0, to.1),
            };
            self.rt.CreateLinearGradientBrush(&props, None, &coll).ok()
        }
    }

    /// Rounded rectangle filled with a linear gradient from `from` to `to`.
    pub fn fill_rr_linear(
        &self,
        r: D2D_RECT_F,
        radius: f32,
        from: (f32, f32),
        to: (f32, f32),
        stops: &[(f32, D2D1_COLOR_F)],
    ) {
        let Some(b) = self.linear_brush(from, to, stops) else {
            return;
        };
        let rr = D2D1_ROUNDED_RECT {
            rect: r,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.rt.FillRoundedRectangle(&rr, &b) };
    }

    /// Rounded-rectangle outline drawn with a linear gradient.
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_rr_linear(
        &self,
        r: D2D_RECT_F,
        radius: f32,
        w: f32,
        from: (f32, f32),
        to: (f32, f32),
        stops: &[(f32, D2D1_COLOR_F)],
    ) {
        let Some(b) = self.linear_brush(from, to, stops) else {
            return;
        };
        let rr = D2D1_ROUNDED_RECT {
            rect: r,
            radiusX: radius,
            radiusY: radius,
        };
        unsafe { self.rt.DrawRoundedRectangle(&rr, &b, w, None) };
    }

    /// Circle outline drawn with a linear gradient (glass rim on round buttons).
    pub fn ring_linear(&self, cx: f32, cy: f32, r: f32, w: f32, stops: &[(f32, D2D1_COLOR_F)]) {
        let Some(b) = self.linear_brush((cx, cy - r), (cx, cy + r), stops) else {
            return;
        };
        let e = D2D1_ELLIPSE {
            point: Vector2::new(cx, cy),
            radiusX: r,
            radiusY: r,
        };
        unsafe { self.rt.DrawEllipse(&e, &b, w, None) };
    }

    /// Filled polygon (straight edges), e.g. small vector icons.
    pub fn fill_poly(&self, pts: &[(f32, f32)], c: D2D1_COLOR_F) {
        let Some(&first) = pts.first() else { return };
        self.set(c);
        unsafe {
            let Ok(geo) = self.g.d2d.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = geo.Open() else { return };
            sink.BeginFigure(Vector2::new(first.0, first.1), D2D1_FIGURE_BEGIN_FILLED);
            for &(x, y) in &pts[1..] {
                sink.AddLine(Vector2::new(x, y));
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
            self.rt.FillGeometry(&geo, self.br, None);
        }
    }

    /// Ellipse filled with a vertical linear gradient (top → bottom), e.g. a gloss highlight.
    pub fn ellipse_linear(
        &self,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        stops: &[(f32, D2D1_COLOR_F)],
    ) {
        let Some(b) = self.linear_brush((cx, cy - ry), (cx, cy + ry), stops) else {
            return;
        };
        let e = D2D1_ELLIPSE {
            point: Vector2::new(cx, cy),
            radiusX: rx,
            radiusY: ry,
        };
        unsafe { self.rt.FillEllipse(&e, &b) };
    }

    /// The Bosun icon's glass recipe on a rounded rectangle, in five smooth layers:
    /// body (lifted top → blue-lit base), a soft top gloss, light pooling at the
    /// bottom, a faint diagonal reflection, and a crisp edge highlight.
    /// `glow` (0..1) sets the blue intensity; `alpha` < 1 lets the background through.
    pub fn glass_rr(&self, r: D2D_RECT_F, radius: f32, base: (u32, u32), glow: f32, alpha: f32) {
        const BLUE: u32 = 0x1E9CFF;
        let (top, bottom, x) = (r.top, r.bottom, (r.left + r.right) / 2.0);
        let v = ((x, top), (x, bottom));
        let lifted = D2D1_COLOR_F {
            a: alpha,
            ..mix(base.0, 0xFFFFFF, 0.04)
        };
        let foot = D2D1_COLOR_F {
            a: alpha,
            ..mix(base.1, BLUE, 0.08 * glow)
        };
        self.fill_rr_linear(
            r,
            radius,
            v.0,
            v.1,
            &[(0.0, lifted), (0.55, cola(base.1, alpha)), (1.0, foot)],
        );
        // gloss: a gentle lift at the very top, gone by the middle
        let gloss = [
            (0.0, cola(0xFFFFFF, 0.07)),
            (0.18, cola(0xFFFFFF, 0.025)),
            (0.55, cola(0xFFFFFF, 0.0)),
        ];
        self.fill_rr_linear(r, radius, v.0, v.1, &gloss);
        // a little light pooling at the bottom edge
        let pool = [
            (0.0, cola(BLUE, 0.0)),
            (0.62, cola(BLUE, 0.0)),
            (1.0, cola(BLUE, 0.10 * glow)),
        ];
        self.fill_rr_linear(r, radius, v.0, v.1, &pool);
        // edge highlight: white along the top, light blue along the bottom
        let inset = D2D_RECT_F {
            left: r.left + 0.5,
            top: r.top + 0.5,
            right: r.right - 0.5,
            bottom: r.bottom - 0.5,
        };
        let edge = [
            (0.0, cola(0xFFFFFF, 0.22)),
            (0.06, cola(0xFFFFFF, 0.08)),
            (0.5, cola(0xFFFFFF, 0.04)),
            (1.0, cola(0xA8DCFF, 0.25 * glow)),
        ];
        self.stroke_rr_linear(inset, radius - 0.5, 1.0, v.0, v.1, &edge);
    }

    /// Film grain over `r`: a tiled 128 px noise texture of light and dark
    /// specks at `strength` opacity. Breaks up gradient banding and adds texture.
    pub fn grain(&self, r: D2D_RECT_F, strength: f32) {
        const N: usize = 128;
        let mut px = vec![0u8; N * N * 4];
        let mut s: u32 = 0x9E37_79B9;
        for q in px.as_chunks_mut::<4>().0 {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let a = ((s & 0xFF) as f32 * strength) as u8;
            // premultiplied BGRA: white specks (v = a) or black specks (v = 0)
            let v = if s & 0x100 != 0 { a } else { 0 };
            *q = [v, v, v, a];
        }
        let props = windows::Win32::Graphics::Direct2D::D2D1_BITMAP_PROPERTIES {
            pixelFormat: windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode:
                    windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        let bp = windows::Win32::Graphics::Direct2D::D2D1_BITMAP_BRUSH_PROPERTIES {
            extendModeX: windows::Win32::Graphics::Direct2D::D2D1_EXTEND_MODE_WRAP,
            extendModeY: windows::Win32::Graphics::Direct2D::D2D1_EXTEND_MODE_WRAP,
            interpolationMode:
                windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
        };
        // SAFETY: `px` outlives CreateBitmap, which copies it; brush is temporary.
        unsafe {
            let size = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U {
                width: N as u32,
                height: N as u32,
            };
            let Ok(bmp) =
                self.rt
                    .CreateBitmap(size, Some(px.as_ptr().cast()), (N * 4) as u32, &props)
            else {
                return;
            };
            let Ok(brush) = self.rt.CreateBitmapBrush(&bmp, Some(&bp), None) else {
                return;
            };
            self.rt.FillRectangle(&r, &brush);
        }
    }

    /// Filled circle with a radial gradient; `stops` are (position 0..1, colour).
    pub fn radial(&self, cx: f32, cy: f32, r: f32, stops: &[(f32, D2D1_COLOR_F)]) {
        self.radial_at((cx, cy, r), (cx, cy, r), stops);
    }

    /// Fills the circle `fill` with a radial gradient centred/sized by `grad`
    /// (lets a highlight sit off-centre while staying inside the circle).
    pub fn radial_at(
        &self,
        fill: (f32, f32, f32),
        grad: (f32, f32, f32),
        stops: &[(f32, D2D1_COLOR_F)],
    ) {
        let (cx, cy, r) = fill;
        let (gx, gy, gr) = grad;
        let gs: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|&(position, color)| D2D1_GRADIENT_STOP { position, color })
            .collect();
        // SAFETY: temporary brush resources on the current render target.
        unsafe {
            let Ok(coll) =
                self.rt
                    .CreateGradientStopCollection(&gs, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
            else {
                return;
            };
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: Vector2::new(gx, gy),
                gradientOriginOffset: Vector2::new(0.0, 0.0),
                radiusX: gr,
                radiusY: gr,
            };
            let Ok(brush) = self.rt.CreateRadialGradientBrush(&props, None, &coll) else {
                return;
            };
            let e = D2D1_ELLIPSE {
                point: Vector2::new(cx, cy),
                radiusX: r,
                radiusY: r,
            };
            self.rt.FillEllipse(&e, &brush);
        }
    }

    /// Cubic Bézier stroke from `p0` to `p3` via controls `c1`, `c2`, round caps.
    pub fn curve(&self, pts: [(f32, f32); 4], w: f32, c: D2D1_COLOR_F) {
        let v = |(x, y): (f32, f32)| Vector2::new(x, y);
        self.set(c);
        unsafe {
            let Ok(geo) = self.g.d2d.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = geo.Open() else { return };
            sink.BeginFigure(v(pts[0]), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: v(pts[1]),
                point2: v(pts[2]),
                point3: v(pts[3]),
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
            self.rt.DrawGeometry(&geo, self.br, w, &self.g.round);
        }
    }

    /// Arc segment with flat ends, from `a0` to `a1` (fractions of a turn from 12 o'clock).
    #[allow(clippy::too_many_arguments)]
    pub fn arc_flat(&self, cx: f32, cy: f32, r: f32, w: f32, a0: f32, a1: f32, c: D2D1_COLOR_F) {
        let (t0, t1) = (a0 * std::f32::consts::TAU, a1 * std::f32::consts::TAU);
        let pt = |t: f32| Vector2::new(cx + r * t.sin(), cy - r * t.cos());
        self.set(c);
        unsafe {
            let Ok(geo) = self.g.d2d.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = geo.Open() else { return };
            sink.BeginFigure(pt(t0), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: pt(t1),
                size: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_F {
                    width: r,
                    height: r,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if a1 - a0 > 0.5 {
                    D2D1_ARC_SIZE_LARGE
                } else {
                    D2D1_ARC_SIZE_SMALL
                },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
            self.rt.DrawGeometry(&geo, self.br, w, None);
        }
    }

    pub fn bitmap(&self, b: &ID2D1Bitmap, r: D2D_RECT_F, opacity: f32) {
        unsafe {
            self.rt.DrawBitmap(
                b,
                Some(&r),
                opacity,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            )
        };
    }

    pub fn clip(&self, r: D2D_RECT_F) {
        unsafe {
            self.rt
                .PushAxisAlignedClip(&r, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
        };
    }

    pub fn unclip(&self) {
        unsafe { self.rt.PopAxisAlignedClip() };
    }
}

/// Physical pixels → DIPs for a given DPI.
pub fn dip(px: i32, dpi: u32) -> f32 {
    px as f32 * 96.0 / dpi as f32
}

/// DIPs → physical pixels for a given DPI.
pub fn px(d: f32, dpi: u32) -> i32 {
    (d * dpi as f32 / 96.0).round() as i32
}

pub fn rect_size(r: &RECT) -> (i32, i32) {
    (r.right - r.left, r.bottom - r.top)
}
