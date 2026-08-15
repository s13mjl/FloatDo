//! GDI 绘制辅助函数（字体、颜色、圆角矩形、逐像素悬浮球、托盘图标）。
//! 针对 windows crate 0.58 的实际 API 签名。

use windows::Win32::Foundation::{BOOL, COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO, HICON, UpdateLayeredWindow, ULW_ALPHA};
use windows::core::w;

#[inline]
pub fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(((b as u32) << 16) | ((g as u32) << 8) | (r as u32))
}

pub unsafe fn make_font(height: i32, bold: bool, strike: bool) -> HFONT {
    CreateFontW(
        height,
        0, 0, 0,
        if bold { 700 } else { 400 },
        0, 0,
        if strike { 1 } else { 0 },
        1,      // DEFAULT_CHARSET
        0, 0,   // OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS
        5,      // CLEARTYPE_QUALITY
        0,      // DEFAULT_PITCH | FF_DONTCARE
        w!("Microsoft YaHei"),
    )
}

pub unsafe fn draw_text(
    hdc: HDC,
    text: &str,
    rect: RECT,
    color: COLORREF,
    flags: DRAW_TEXT_FORMAT,
    font: HFONT,
) {
    let old = SelectObject(hdc, font);
    SetTextColor(hdc, color);
    SetBkMode(hdc, TRANSPARENT);
    let mut wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let mut r = rect;
    DrawTextW(hdc, &mut wide, &mut r, flags);
    SelectObject(hdc, old);
}

/// 填充 + 描边的圆角矩形。
pub unsafe fn round_rect(
    hdc: HDC,
    rect: RECT,
    radius: i32,
    fill: COLORREF,
    border_color: COLORREF,
    border_width: i32,
) {
    let brush = CreateSolidBrush(fill);
    let pen = CreatePen(PS_SOLID, border_width, border_color);
    let ob = SelectObject(hdc, brush);
    let op = SelectObject(hdc, pen);
    RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, radius, radius);
    SelectObject(hdc, op);
    SelectObject(hdc, ob);
    DeleteObject(pen);
    DeleteObject(brush);
}

/// 填充椭圆（无描边）。
pub unsafe fn fill_ellipse(hdc: HDC, rect: RECT, color: COLORREF) {
    let brush = CreateSolidBrush(color);
    let ob = SelectObject(hdc, brush);
    let op = SelectObject(hdc, GetStockObject(NULL_PEN));
    Ellipse(hdc, rect.left, rect.top, rect.right, rect.bottom);
    SelectObject(hdc, op);
    SelectObject(hdc, ob);
    DeleteObject(brush);
}

/// 将 BGRA 像素图（含 alpha）绘制到目标 DC（用于彩色 emoji）。
pub unsafe fn blit_bgra(hdc: HDC, dst: RECT, px: &[u8], w: i32, h: i32) {
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h; // 自上而下
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0 as u32;

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let hdc_screen = GetDC(None);
    let hbmp = CreateDIBSection(hdc_screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
    if let Ok(hbmp) = hbmp {
        if !bits.is_null() {
            std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u8, px.len());
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let old = SelectObject(hdc_mem, hbmp);
        let blend = BLENDFUNCTION {
            BlendOp: 0,       // AC_SRC_OVER
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1,   // AC_SRC_ALPHA
        };
        let dw = dst.right - dst.left;
        let dh = dst.bottom - dst.top;
        AlphaBlend(hdc, dst.left, dst.top, dw, dh, hdc_mem, 0, 0, w, h, blend);
        SelectObject(hdc_mem, old);
        DeleteObject(hbmp);
        DeleteDC(hdc_mem);
    }
    ReleaseDC(None, hdc_screen);
}

/// 渲染悬浮球像素（预乘 BGRA）。breath ∈ [0,1] 光晕脉动，anim_scale 缩放动画。
pub fn render_ball(size: i32, breath: f32, anim_scale: f32) -> Vec<u32> {
    let mut px = vec![0u32; (size * size) as usize];
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let scale = size as f32 / 100.0;
    let base_r = 28.0 * scale * anim_scale;
    let glow_r = base_r + (5.0 + 4.0 * breath) * scale;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let d = (dx * dx + dy * dy).sqrt();

            let mut a = 0.0f32;
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;

            // 光晕层（紫 → 青渐变）
            if d <= glow_r {
                let t = (d / glow_r).clamp(0.0, 1.0);
                let ga = (1.0 - t).powf(1.6) * 0.55;
                r = 108.0 - 108.0 * t;
                g = 92.0 + 76.0 * t;
                b = 231.0 + 24.0 * t;
                a = ga;
            }

            // 球体层
            if d <= base_r {
                let t = (d / base_r).clamp(0.0, 1.0);
                let sr = 40.0 + 90.0 * (1.0 - t);
                let sg = 40.0 + 70.0 * (1.0 - t);
                let sb = 70.0 + 180.0 * (1.0 - t);
                a = 1.0 - 0.15 * t;
                r = sr;
                g = sg;
                b = sb;
                // 左上高光
                let hx = cx - base_r * 0.35;
                let hy = cy - base_r * 0.45;
                let hd = ((x as f32 - hx).powi(2) + (y as f32 - hy).powi(2)).sqrt();
                let hr = base_r * 0.45;
                if hd <= hr {
                    let ht = (hd / hr).clamp(0.0, 1.0);
                    let ha = (1.0 - ht) * 0.5;
                    r = (r + 255.0 * ha).min(255.0);
                    g = (g + 255.0 * ha).min(255.0);
                    b = (b + 255.0 * ha).min(255.0);
                }
            }

            let ai = (a.clamp(0.0, 1.0) * 255.0) as u32;
            let ri = ((r * a).clamp(0.0, 255.0)) as u32;
            let gi = ((g * a).clamp(0.0, 255.0)) as u32;
            let bi = ((b * a).clamp(0.0, 255.0)) as u32;
            px[(y * size + x) as usize] = (ai << 24) | (ri << 16) | (gi << 8) | bi;
        }
    }
    px
}

/// 用逐像素 alpha 更新分层窗口（悬浮球）。
pub unsafe fn push_layered(hwnd: HWND, px: &[u32], w: i32, h: i32) {
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h; // 自上而下
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0 as u32;

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let hdc_screen = GetDC(None);
    let hbmp = CreateDIBSection(hdc_screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
    if let Ok(hbmp) = hbmp {
        if !bits.is_null() {
            std::ptr::copy_nonoverlapping(px.as_ptr() as *const u8, bits as *mut u8, px.len() * 4);
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let old = SelectObject(hdc_mem, hbmp);
        let pt_src = POINT { x: 0, y: 0 };
        // 用窗口当前位置作为 pt_dst，不要固定 (0,0)
        let mut rc = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rc);
        let pt_dst = POINT { x: rc.left, y: rc.top };
        let sz = SIZE { cx: w, cy: h };
        let blend = BLENDFUNCTION {
            BlendOp: 0,       // AC_SRC_OVER
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1,   // AC_SRC_ALPHA
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            None,
            Some(&pt_dst),
            Some(&sz),
            hdc_mem,
            Some(&pt_src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        SelectObject(hdc_mem, old);
        DeleteObject(hbmp);
        DeleteDC(hdc_mem);
    }
    ReleaseDC(None, hdc_screen);
}

/// 生成托盘图标（32x32 紫色圆点 + 白色高光）。
/// 关键：AND 掩码 黑(0)=不透明、白(1)=透明；颜色位图直接写 BGRA 带 alpha。
pub unsafe fn make_tray_icon() -> HICON {
    let size: i32 = 32;

    // 1) 颜色位图：直接写 BGRA 像素（含 alpha），紫圆不透明、背景透明
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = size;
    bmi.bmiHeader.biHeight = -size; // 自上而下
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0 as u32;

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let hdc_screen = GetDC(None);
    let hbmp = CreateDIBSection(hdc_screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();

    if !bits.is_null() {
        let px = bits as *mut u32;
        let c = size as f32 / 2.0;
        let r = size as f32 / 2.0 - 2.0; // 圆半径（留 2px 边距）
        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 + 0.5 - c;
                let dy = y as f32 + 0.5 - c;
                let d = (dx * dx + dy * dy).sqrt();
                // 默认透明（alpha=0）
                let mut v: u32 = 0;
                if d <= r {
                    // 紫色 RGB(108,92,231)
                    let (mut rr, mut gg, mut bb) = (108u32, 92u32, 231u32);
                    // 左上白色高光
                    let hx = c - r * 0.35;
                    let hy = c - r * 0.45;
                    let hd = ((x as f32 - hx).powi(2) + (y as f32 - hy).powi(2)).sqrt();
                    if hd <= r * 0.42 {
                        rr = 255;
                        gg = 255;
                        bb = 255;
                    }
                    // 存 BGRA（小端内存顺序）
                    v = (0xFFu32 << 24) | (rr << 16) | (gg << 8) | bb;
                }
                *px.offset((y * size + x) as isize) = v;
            }
        }
    }

    // 2) 单色 AND 掩码：黑(0)=不透明、白(1)=透明
    //    逐像素写，与颜色圆用完全相同的判定，保证掩码和颜色 100% 一致
    let mask = CreateBitmap(size, size, 1, 1, None);
    {
        let c = size as f32 / 2.0;
        let r = size as f32 / 2.0 - 2.0;
        // 每行按 4 字节对齐（1bpp，size=32 正好一行 4 字节）
        let row_bytes = ((size + 31) / 32) * 4;
        let mut mask_bits = vec![0u8; (row_bytes * size) as usize];
        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 + 0.5 - c;
                let dy = y as f32 + 0.5 - c;
                let d = (dx * dx + dy * dy).sqrt();
                // 圆内 = 不透明 = 位 0（黑）；圆外 = 透明 = 位 1（白）
                // 1bpp 位图：bit 1 = 白色 = 透明，bit 0 = 黑色 = 不透明
                if d > r {
                    // 透明 → 位设为 1
                    let byte_idx = (y * row_bytes + x / 8) as usize;
                    let bit = 7 - (x % 8);
                    mask_bits[byte_idx] |= 1u8 << bit;
                }
            }
        }
        // 用 SetDIBits 写入掩码位图
        let mut mask_bmi = BITMAPINFO::default();
        mask_bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        mask_bmi.bmiHeader.biWidth = size;
        mask_bmi.bmiHeader.biHeight = -size; // 自上而下
        mask_bmi.bmiHeader.biPlanes = 1;
        mask_bmi.bmiHeader.biBitCount = 1;
        mask_bmi.bmiHeader.biCompression = BI_RGB.0 as u32;
        let hdc_tmp = CreateCompatibleDC(hdc_screen);
        SetDIBits(
            hdc_tmp,
            mask,
            0,
            size as u32,
            mask_bits.as_ptr() as *const core::ffi::c_void,
            &mask_bmi,
            DIB_RGB_COLORS,
        );
        DeleteDC(hdc_tmp);
    }

    let iconinfo = ICONINFO {
        fIcon: BOOL(1),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: hbmp,
    };
    let icon = CreateIconIndirect(&iconinfo).unwrap();

    DeleteObject(mask);
    DeleteObject(hbmp);
    ReleaseDC(None, hdc_screen);

    icon
}
