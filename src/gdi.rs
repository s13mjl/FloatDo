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

/// 在矩形内居中绘制矢量感叹号（抗锯齿），用于重要性标记。
/// 不依赖字体字形，避免字形前伸距导致的视觉偏移。
pub unsafe fn draw_bang_aa(
    px: *mut u32,
    stride: i32,
    clip: RECT,
    rect: RECT,
    color: COLORREF,
) {
    let unit = (rect.bottom - rect.top) as f32 / 18.0;
    // 竖条与圆点共用同一个整数中心，避免取整误差造成圆点偏左
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;
    let half = ((2.0 * unit).round() as i32).max(1);
    let bar = RECT {
        left: cx - half,
        top: cy - (6.4 * unit).round() as i32,
        right: cx + half,
        bottom: cy + (0.6 * unit).round() as i32,
    };
    round_rect_aa(px, stride, clip, bar, half as f32, color, color, 0.0);

    let dot_cy = cy + (4.6 * unit).round() as i32;
    let dot = RECT {
        left: cx - half,
        top: dot_cy - half,
        right: cx + half,
        bottom: dot_cy + half,
    };
    fill_ellipse_aa(px, stride, clip, dot, color);
}

/// 在指定坐标绘制单行文本（不做矩形裁剪，适合按字节宽精确定位的短文本）。
pub unsafe fn text_out(hdc: HDC, text: &str, x: i32, y: i32, color: COLORREF, font: HFONT) {
    let old = SelectObject(hdc, font);
    SetTextColor(hdc, color);
    SetBkMode(hdc, TRANSPARENT);
    let wide: Vec<u16> = text.encode_utf16().collect();
    TextOutW(hdc, x, y, &wide);
    SelectObject(hdc, old);
}

/// 填充 + 描边的抗锯齿圆角矩形（逐像素写入 DIB，clip 为裁剪区域）。
pub unsafe fn round_rect_aa(
    px: *mut u32,
    stride: i32,
    clip: RECT,
    rect: RECT,
    radius: f32,
    fill: COLORREF,
    border: COLORREF,
    border_w: f32,
) {
    let x0 = rect.left.max(clip.left).max(0);
    let y0 = rect.top.max(clip.top).max(0);
    let x1 = rect.right.min(clip.right);
    let y1 = rect.bottom.min(clip.bottom);
    let cx = (rect.left + rect.right) as f32 * 0.5;
    let cy = (rect.top + rect.bottom) as f32 * 0.5;
    let hw = ((rect.right - rect.left) as f32 * 0.5 - radius).max(0.0);
    let hh = ((rect.bottom - rect.top) as f32 * 0.5 - radius).max(0.0);
    let inner_r = (radius - border_w).max(0.0);
    let (br, bg, bb) = unpack(border);
    let (fr, fg, fb) = unpack(fill);
    for y in y0..y1 {
        for x in x0..x1 {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let idx = (y * stride + x) as usize;
            let cov_out = edge_cov(sdf_round_rect(fx, fy, cx, cy, hw, hh, radius),
                || sub_cov_round_rect(x as f32, y as f32, cx, cy, hw, hh, radius));
            if cov_out > 0.0 {
                blend_px(px, idx, br, bg, bb, cov_out);
            }
            let cov_in = edge_cov(sdf_round_rect(fx, fy, cx, cy, hw, hh, inner_r),
                || sub_cov_round_rect(x as f32, y as f32, cx, cy, hw, hh, inner_r));
            if cov_in > 0.0 {
                blend_px(px, idx, fr, fg, fb, cov_in);
            }
        }
    }
}

/// 抗锯齿填充椭圆。
pub unsafe fn fill_ellipse_aa(px: *mut u32, stride: i32, clip: RECT, rect: RECT, color: COLORREF) {
    let x0 = rect.left.max(clip.left).max(0);
    let y0 = rect.top.max(clip.top).max(0);
    let x1 = rect.right.min(clip.right);
    let y1 = rect.bottom.min(clip.bottom);
    let cx = (rect.left + rect.right) as f32 * 0.5;
    let cy = (rect.top + rect.bottom) as f32 * 0.5;
    let rx = (rect.right - rect.left) as f32 * 0.5;
    let ry = (rect.bottom - rect.top) as f32 * 0.5;
    let (r, g, b) = unpack(color);
    for y in y0..y1 {
        for x in x0..x1 {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let cov = edge_cov(sdf_ellipse(fx, fy, cx, cy, rx, ry),
                || sub_cov_ellipse(x as f32, y as f32, cx, cy, rx, ry));
            if cov > 0.0 {
                blend_px(px, (y * stride + x) as usize, r, g, b, cov);
            }
        }
    }
}

/// 抗锯齿椭圆描边。
pub unsafe fn stroke_ellipse_aa(
    px: *mut u32,
    stride: i32,
    clip: RECT,
    rect: RECT,
    width: f32,
    color: COLORREF,
) {
    // 椭圆矩形即描边外缘：路径半径向内收一半笔宽，与 GDI 画笔的绘制范围一致
    let half = width * 0.5;
    let x0 = rect.left.max(clip.left).max(0);
    let y0 = rect.top.max(clip.top).max(0);
    let x1 = rect.right.min(clip.right);
    let y1 = rect.bottom.min(clip.bottom);
    let cx = (rect.left + rect.right) as f32 * 0.5;
    let cy = (rect.top + rect.bottom) as f32 * 0.5;
    let prx = ((rect.right - rect.left) as f32 * 0.5 - half).max(1.0);
    let pry = ((rect.bottom - rect.top) as f32 * 0.5 - half).max(1.0);
    let (r, g, b) = unpack(color);
    for y in y0..y1 {
        for x in x0..x1 {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let cov = edge_cov(sdf_ellipse(fx, fy, cx, cy, prx, pry).abs() - half,
                || sub_cov_ring(x as f32, y as f32, cx, cy, prx, pry, half));
            if cov > 0.0 {
                blend_px(px, (y * stride + x) as usize, r, g, b, cov);
            }
        }
    }
}

/// 像素中心离形状边缘超过 0.8px 时可直接判定 0/1，只有边界像素做超采样。
#[inline]
fn edge_cov(sd: f32, sample: impl FnOnce() -> f32) -> f32 {
    if sd <= -0.8 {
        1.0
    } else if sd >= 0.8 {
        0.0
    } else {
        sample()
    }
}

const AA_N: i32 = 8;

/// 8x8 超采样：圆角矩形精确包含判定。
fn sub_cov_round_rect(x: f32, y: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let step = 1.0 / AA_N as f32;
    let mut hit = 0;
    for sy in 0..AA_N {
        let py = y + (sy as f32 + 0.5) * step;
        for sx in 0..AA_N {
            let px = x + (sx as f32 + 0.5) * step;
            let ax = ((px - cx).abs() - hw).max(0.0);
            let ay = ((py - cy).abs() - hh).max(0.0);
            if ax * ax + ay * ay <= r * r {
                hit += 1;
            }
        }
    }
    hit as f32 / (AA_N * AA_N) as f32
}

/// 8x8 超采样：椭圆包含判定。
fn sub_cov_ellipse(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    let step = 1.0 / AA_N as f32;
    let mut hit = 0;
    for sy in 0..AA_N {
        let ny = (y + (sy as f32 + 0.5) * step - cy) / ry;
        for sx in 0..AA_N {
            let nx = (x + (sx as f32 + 0.5) * step - cx) / rx;
            if nx * nx + ny * ny <= 1.0 {
                hit += 1;
            }
        }
    }
    hit as f32 / (AA_N * AA_N) as f32
}

/// 8x8 超采样：椭圆环带判定（|到椭圆距离| <= half）。
fn sub_cov_ring(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32, half: f32) -> f32 {
    let step = 1.0 / AA_N as f32;
    let mut hit = 0;
    for sy in 0..AA_N {
        let py = y + (sy as f32 + 0.5) * step;
        for sx in 0..AA_N {
            let px = x + (sx as f32 + 0.5) * step;
            if sdf_ellipse(px, py, cx, cy, rx, ry).abs() <= half {
                hit += 1;
            }
        }
    }
    hit as f32 / (AA_N * AA_N) as f32
}

#[inline]
fn unpack(c: COLORREF) -> (u32, u32, u32) {
    (c.0 & 0xFF, (c.0 >> 8) & 0xFF, (c.0 >> 16) & 0xFF)
}

#[inline]
fn sdf_round_rect(x: f32, y: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (x - cx).abs() - hw;
    let qy = (y - cy).abs() - hh;
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
}

#[inline]
fn sdf_ellipse(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    let nx = (x - cx) / rx;
    let ny = (y - cy) / ry;
    ((nx * nx + ny * ny).sqrt() - 1.0) * rx.min(ry)
}

/// 以覆盖率把颜色混入 DIB 像素（内存序 B G R A，保留 alpha 字节）。
#[inline]
unsafe fn blend_px(px: *mut u32, idx: usize, r: u32, g: u32, b: u32, cov: f32) {
    let v = *px.add(idx);
    let db = (v & 0xFF) as f32;
    let dg = ((v >> 8) & 0xFF) as f32;
    let dr = ((v >> 16) & 0xFF) as f32;
    let nb = (db + (b as f32 - db) * cov + 0.5).min(255.0) as u32;
    let ng = (dg + (g as f32 - dg) * cov + 0.5).min(255.0) as u32;
    let nr = (dr + (r as f32 - dr) * cov + 0.5).min(255.0) as u32;
    *px.add(idx) = (v & 0xFF00_0000) | (nr << 16) | (ng << 8) | nb;
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

/// 圆角抗锯齿：内部像素 alpha=255；四个角落按圆弧覆盖率渐变写 alpha（RGB 同步预乘）。
pub unsafe fn round_corner_alpha(px: *mut u32, w: i32, h: i32, r: i32) {
    let rf = r as f32;
    for y in 0..h {
        for x in 0..w {
            let p = px.offset((y * w + x) as isize);
            let in_corner = (x < r || x >= w - r) && (y < r || y >= h - r);
            if !in_corner {
                *p |= 0xFF00_0000;
                continue;
            }
            let ccx = if x < r { rf } else { (w - r) as f32 };
            let ccy = if y < r { rf } else { (h - r) as f32 };
            let dx = x as f32 + 0.5 - ccx;
            let dy = y as f32 + 0.5 - ccy;
            let d = (dx * dx + dy * dy).sqrt();
            let cov = (rf + 0.5 - d).clamp(0.0, 1.0);
            let v = *p;
            let bp = ((v & 0xFF) as f32 * cov) as u32;
            let gp = (((v >> 8) & 0xFF) as f32 * cov) as u32;
            let rp = (((v >> 16) & 0xFF) as f32 * cov) as u32;
            let ap = (cov * 255.0) as u32;
            *p = (ap << 24) | (rp << 16) | (gp << 8) | bp;
        }
    }
}

/// 点到线段的距离。
#[inline]
fn dist_to_seg(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let vx = b.0 - a.0;
    let vy = b.1 - a.1;
    let wx = px - a.0;
    let wy = py - a.1;
    let len2 = vx * vx + vy * vy;
    let t = if len2 > 0.0 {
        ((wx * vx + wy * vy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = wx - vx * t;
    let dy = wy - vy * t;
    (dx * dx + dy * dy).sqrt()
}

/// 抗锯齿的 V 形箭头（< / >），point_left 表示箭头朝左。
pub unsafe fn draw_chevron_aa(
    px: *mut u32,
    stride: i32,
    clip: RECT,
    rect: RECT,
    point_left: bool,
    width: f32,
    color: COLORREF,
) {
    let cx = (rect.left + rect.right) as f32 * 0.5;
    let cy = (rect.top + rect.bottom) as f32 * 0.5;
    let half_h = 7.5;
    let half_w = 5.0;
    let tip_x = if point_left { cx - half_w } else { cx + half_w };
    let arm_x = if point_left { cx + half_w } else { cx - half_w };
    let tip = (tip_x, cy);
    let arm_top = (arm_x, cy - half_h);
    let arm_bot = (arm_x, cy + half_h);
    let (r, g, b) = unpack(color);
    let half = width * 0.5;
    let x0 = rect.left.max(clip.left).max(0);
    let y0 = rect.top.max(clip.top).max(0);
    let x1 = rect.right.min(clip.right);
    let y1 = rect.bottom.min(clip.bottom);
    for y in y0..y1 {
        for x in x0..x1 {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let d = dist_to_seg(fx, fy, tip, arm_top).min(dist_to_seg(fx, fy, tip, arm_bot));
            let cov = (half + 0.5 - d).clamp(0.0, 1.0);
            if cov > 0.0 {
                blend_px(px, (y * stride + x) as usize, r, g, b, cov);
            }
        }
    }
}

/// 渲染悬浮球像素（预乘 BGRA）。breath ∈ [0,1] 光晕脉动，anim_scale 缩放动画。
/// 边缘按 1px 覆盖率抗锯齿，避免圆形轮廓出现台阶。
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

            // 半径 r 的圆在该像素上的覆盖度（0..1，边缘 1px 渐变）
            let cov = |r: f32| (r + 0.5 - d).clamp(0.0, 1.0);

            // 光晕层（紫 → 青渐变）
            let t = (d / glow_r).clamp(0.0, 1.0);
            let ga = (1.0 - t).powf(1.6) * 0.55 * cov(glow_r);
            let gr = 108.0 - 108.0 * t;
            let gg = 92.0 + 76.0 * t;
            let gb = 231.0 + 24.0 * t;

            // 球体层
            let bcov = cov(base_r);
            let mut ba = 0.0f32;
            let (mut br, mut bg, mut bb) = (0.0f32, 0.0f32, 0.0f32);
            if bcov > 0.0 {
                let tb = (d / base_r).clamp(0.0, 1.0);
                br = 40.0 + 90.0 * (1.0 - tb);
                bg = 40.0 + 70.0 * (1.0 - tb);
                bb = 70.0 + 180.0 * (1.0 - tb);
                ba = (1.0 - 0.15 * tb) * bcov;

                // 左上高光（径向已平滑衰减至 0，无需再做边缘覆盖）
                let hx = cx - base_r * 0.35;
                let hy = cy - base_r * 0.45;
                let hd = ((x as f32 - hx).powi(2) + (y as f32 - hy).powi(2)).sqrt();
                let hr = base_r * 0.45;
                let ht = (hd / hr).clamp(0.0, 1.0);
                let ha = (1.0 - ht) * 0.5;
                br = (br + 255.0 * ha).min(255.0);
                bg = (bg + 255.0 * ha).min(255.0);
                bb = (bb + 255.0 * ha).min(255.0);
            }

            // 球体 over 光晕，按预乘 alpha 存储
            let ai = ((ba + ga * (1.0 - ba)).clamp(0.0, 1.0) * 255.0) as u32;
            let ri = ((br * ba + gr * ga * (1.0 - ba)).clamp(0.0, 255.0)) as u32;
            let gi = ((bg * ba + gg * ga * (1.0 - ba)).clamp(0.0, 255.0)) as u32;
            let bi = ((bb * ba + gb * ga * (1.0 - ba)).clamp(0.0, 255.0)) as u32;
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

