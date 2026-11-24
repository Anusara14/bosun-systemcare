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
    dpi: u32,
}

thread_local! {
    /// One render target for every surface on this thread. Software targets
    /// run on WARP, which compiles a shader per kind of shape on each device:
    /// sharing the device means each is compiled once (MBs, not per window).
    static SHARED: std::cell::RefCell<Option<(ID2D1DCRenderTarget, ID2D1SolidColorBrush)>> =
        const { std::cell::RefCell::new(None) };
}

fn shared() -> Result<(ID2D1DCRenderTarget, ID2D1SolidColorBrush)> {
    SHARED.with(|s| {
        if let Some(t) = s.borrow().as_ref() {
            return Ok(t.clone());
        }
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        // SAFETY: plain factory calls; the target lives for the thread.
        let t = unsafe {
            let rt = graphics().d2d.CreateDCRenderTarget(&props)?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let br = rt.CreateSolidColorBrush(&col(tok::TEXT), None)?;
            (rt, br)
        };
        *s.borrow_mut() = Some(t.clone());
        Ok(t)
    })
}

/// The thread's shared render target, for resources drawn through it
/// (bitmaps made with `Gfx::icon_bitmap`).
pub fn shared_target() -> Option<ID2D1RenderTarget> {
    shared().ok().map(|(rt, _)| rt.into())
}

/// Gives back what the shared target holds (WARP's device and its compiled
/// shaders) and returns freed heap pages to Windows. For when a big window
/// such as the monitor closes; the next frame rebuilds a small target.
pub fn release() {
    reset_shared();
    // SAFETY: compacting our own process heap; no pointers are invalidated.
    unsafe {
        if let Ok(h) = windows::Win32::System::Memory::GetProcessHeap() {
            let _ = windows::Win32::System::Memory::HeapCompact(h, Default::default());
        }
    }
}

/// Drops the shared target (after a failed draw it is rebuilt on next use).
fn reset_shared() {
    SHARED.with(|s| *s.borrow_mut() = None);
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
            Ok(Self {
                dc,
                dib,
                old,
                w: pw,
                h: ph,
                dpi,
            })
        }
    }

    /// Draws a frame with `f` into the surface only (see `blit`). False if
    /// drawing failed; the shared target is rebuilt on the next frame.
    pub fn draw(&self, f: impl FnOnce(&Painter)) -> bool {
        // SAFETY: binds the shared target to our DIB and draws one frame.
        unsafe {
            let rc = RECT {
                left: 0,
                top: 0,
                right: self.w,
                bottom: self.h,
            };
            let Ok((target, br)) = shared() else {
                return false;
            };
            target.SetDpi(self.dpi as f32, self.dpi as f32);
            if target.BindDC(self.dc, &rc).is_err() {
                return false;
            }
            target.BeginDraw();
            let rt: &ID2D1RenderTarget = &target;
            f(&Painter {
                rt,
                br: &br,
                g: graphics(),
            });
            if target.EndDraw(None, None).is_err() {
                reset_shared();
                return false;
            }
        }
        true
    }

    /// Copies the last frame to `hdc` at (0, 0) (for ordinary windows).
    pub fn blit(&self, hdc: HDC) {
        use windows::Win32::Graphics::Gdi::{BitBlt, SRCCOPY};
        // SAFETY: copies between two live DCs.
        unsafe {
            let _ = BitBlt(hdc, 0, 0, self.w, self.h, Some(self.dc), 0, 0, SRCCOPY);
        }
    }

    /// Draws a frame with `f` and pushes it to `hwnd`. Returns false if the
    /// surface must be recreated.
    pub fn present(&self, hwnd: HWND, f: impl FnOnce(&Painter)) -> bool {
        self.present_alpha(hwnd, 255, f)
    }

    /// `present` with the whole window drawn at `alpha` (255 = opaque).
    pub fn present_alpha(&self, hwnd: HWND, alpha: u8, f: impl FnOnce(&Painter)) -> bool {
        if !self.draw(f) {
            return false;
        }
        // SAFETY: hands our finished DIB to the layered window.
        unsafe {
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
