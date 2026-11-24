//! Per-pixel-alpha popups: Direct2D draws into a 32-bit DIB that is handed to
//! `UpdateLayeredWindow`. Shared by the floating widget and the RAM-clean card.

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1DCRenderTarget,
    ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};
use windows::core::Result;

use crate::gfx::{self, Painter, col, tok};
use crate::graphics;

pub struct Surface {
    dc: HDC,
    dib: HBITMAP,
    old: HGDIOBJ,
    pub w: i32,
    pub h: i32,
    rt: ID2D1DCRenderTarget,
    br: ID2D1SolidColorBrush,
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: restores and frees the GDI objects this surface created.
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.dib.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

impl Surface {
    /// A surface of `w × h` DIPs at `dpi`. Software rendering on purpose: a GPU
    /// device would pull ~50 MB of driver into an always-on process.
    pub fn new(w: f32, h: f32, dpi: u32) -> Result<Self> {
        let (pw, ph) = (gfx::px(w, dpi), gfx::px(h, dpi));
        // SAFETY: a top-down 32-bpp DIB selected into its own memory DC; freed in Drop.
        unsafe {
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: pw,
                    biHeight: -ph,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let dib = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let dc = CreateCompatibleDC(None);
            let old = SelectObject(dc, dib.into());
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: dpi as f32,
                dpiY: dpi as f32,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = graphics().d2d.CreateDCRenderTarget(&props)?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let br = rt.CreateSolidColorBrush(&col(tok::TEXT), None)?;
            Ok(Self {
                dc,
                dib,
                old,
                w: pw,
                h: ph,
                rt,
                br,
            })
        }
    }

    /// Draws a frame with `f` and pushes it to `hwnd`. Returns false if the
    /// surface must be recreated.
    pub fn present(&self, hwnd: HWND, f: impl FnOnce(&Painter)) -> bool {
        self.present_alpha(hwnd, 255, f)
    }

    /// `present` with the whole window drawn at `alpha` (255 = opaque).
    pub fn present_alpha(&self, hwnd: HWND, alpha: u8, f: impl FnOnce(&Painter)) -> bool {
        // SAFETY: draws into our DIB, then hands it to the layered window.
        unsafe {
            let rc = RECT {
                left: 0,
                top: 0,
                right: self.w,
                bottom: self.h,
            };
            if self.rt.BindDC(self.dc, &rc).is_err() {
                return false;
            }
            self.rt.BeginDraw();
            let rt: &ID2D1RenderTarget = &self.rt;
            f(&Painter {
                rt,
                br: &self.br,
                g: graphics(),
            });
            if self.rt.EndDraw(None, None).is_err() {
                return false;
            }
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                SourceConstantAlpha: alpha,
                AlphaFormat: AC_SRC_ALPHA as u8,
                ..Default::default()
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                None,
                None,
                Some(&SIZE {
                    cx: self.w,
                    cy: self.h,
                }),
                Some(self.dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
        true
    }
}
