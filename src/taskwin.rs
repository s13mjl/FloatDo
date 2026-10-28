//! 任务窗口 —— 纯 Win32 自绘（无 Edit/ListBox 控件，内存占用最低）。

use crate::gdi::{self, rgb};
use crate::store::TaskStore;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, ReleaseCapture, SetCapture, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

pub const WIN_W: i32 = 380;
pub const WIN_H: i32 = 550;

const TITLE_H: i32 = 40;
const HEADER_H: i32 = 56;
const INPUT_H: i32 = 50;
const CARD_H: i32 = 60;
const CARD_GAP: i32 = 8;
const MARGIN_X: i32 = 15;
const CORNER_R: i32 = 16;
const TIMER_CARET: usize = 1;

const C_PURPLE: (u8, u8, u8) = (108, 92, 231);
const C_WHITE: (u8, u8, u8) = (255, 255, 255);
const C_BORDER: (u8, u8, u8) = (222, 226, 230);
const C_TEXT: (u8, u8, u8) = (45, 52, 54);
const C_MUTED: (u8, u8, u8) = (99, 110, 114);
const C_CARD: (u8, u8, u8) = (248, 249, 250);
const C_CARD_DONE: (u8, u8, u8) = (233, 236, 239);
const C_CHK_BORDER: (u8, u8, u8) = (178, 190, 195);
const C_DEL: (u8, u8, u8) = (178, 190, 195);

pub struct TaskWinState {
    pub hwnd: HWND,
    pub store: Arc<Mutex<TaskStore>>,
    pub input_visible: bool,
    pub input_buffer: String,
    pub drag_off: POINT,
    pub dragging: bool,
    pub font_title: HFONT,
    pub font_header: HFONT,
    pub font_item: HFONT,
    pub font_item_done: HFONT,
    pub font_btn: HFONT,
    pub scroll_y: i32,
    pub caret_on: bool,
}

static mut TASKWIN: *mut TaskWinState = std::ptr::null_mut();

pub fn create(store: Arc<Mutex<TaskStore>>) {
    let class = w!("FloatDoTaskClass");
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(task_wndproc),
            hInstance: GetModuleHandleW(None).unwrap().into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class,
            w!("待办事项"),
            WS_POPUP,
            CW_USEDEFAULT, CW_USEDEFAULT, WIN_W, WIN_H,
            None, None,
            GetModuleHandleW(None).unwrap(),
            None,
        ).unwrap();

        let font_title = gdi::make_font(-20, true, false);
        let font_header = gdi::make_font(-13, true, false);
        let font_item = gdi::make_font(-15, false, false);
        let font_item_done = gdi::make_font(-15, false, true);
        let font_btn = gdi::make_font(-18, true, false);

        let st = Box::leak(Box::new(TaskWinState {
            hwnd,
            store,
            input_visible: false,
            input_buffer: String::new(),
            drag_off: POINT::default(),
            dragging: false,
            font_title,
            font_header,
            font_item,
            font_item_done,
            font_btn,
            scroll_y: 0,
            caret_on: false,
        }));
        TASKWIN = st;
    }
}

pub fn show_current() {
    unsafe {
        if !TASKWIN.is_null() {
            let st = &mut *TASKWIN;
            ShowWindow(st.hwnd, SW_SHOW);
            SetForegroundWindow(st.hwnd);
            SetFocus(st.hwnd);
            reset_caret(st.hwnd, st);
            InvalidateRect(st.hwnd, None, false);
            // 同步画出首帧，避免分层窗口在 UpdateLayeredWindow 之前呈现为透明
            let _ = UpdateWindow(st.hwnd);
        }
    }
}

pub fn hide_current() {
    unsafe {
        if !TASKWIN.is_null() {
            let st = &mut *TASKWIN;
            let _ = KillTimer(st.hwnd, TIMER_CARET);
            st.caret_on = false;
            ShowWindow(st.hwnd, SW_HIDE);
        }
    }
}

unsafe fn st() -> &'static mut TaskWinState {
    &mut *TASKWIN
}

fn list_top(s: &TaskWinState) -> i32 {
    TITLE_H + HEADER_H + if s.input_visible { INPUT_H } else { 0 }
}

fn scroll_max(content_len: usize, view_top: i32) -> i32 {
    let content_h = content_len as i32 * (CARD_H + CARD_GAP);
    let view_h = WIN_H - view_top;
    (content_h - view_h).max(0)
}

/// 鼠标是否位于某张任务卡片的勾选圈或删除按钮上（用于切换手形光标）。
fn over_card_action(s: &TaskWinState, x: i32, y: i32) -> bool {
    let count = s.store.lock().unwrap().all().len() as i32;
    let top = list_top(s) - s.scroll_y;
    for idx in 0..count {
        let y0 = top + idx * (CARD_H + CARD_GAP);
        if y >= y0 && y <= y0 + CARD_H {
            return (x >= MARGIN_X + 15 && x <= MARGIN_X + 15 + 22)
                || (x >= WIN_W - MARGIN_X - 24 - 12 && x <= WIN_W - MARGIN_X);
        }
    }
    false
}

/// 输入框是否处于「可显示插入光标」状态（打开且窗口持有焦点）。
unsafe fn caret_active(hwnd: HWND, s: &TaskWinState) -> bool {
    s.input_visible && GetFocus() == hwnd
}

/// 让插入光标立即可见并重启闪烁计时（打字时保持常亮）。
unsafe fn reset_caret(hwnd: HWND, s: &mut TaskWinState) {
    if !caret_active(hwnd, s) {
        stop_caret(hwnd, s);
        return;
    }
    s.caret_on = true;
    SetTimer(hwnd, TIMER_CARET, 530, None);
    invalidate_input(hwnd);
}

/// 停止闪烁并隐藏插入光标。
unsafe fn stop_caret(hwnd: HWND, s: &mut TaskWinState) {
    let _ = KillTimer(hwnd, TIMER_CARET);
    if s.caret_on {
        s.caret_on = false;
        invalidate_input(hwnd);
    }
}

unsafe extern "system" fn task_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if TASKWIN.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        // 分层窗口只有 UpdateLayeredWindow 过才有内容，显示时必须整窗重绘一次
        WM_SHOWWINDOW => {
            if wparam.0 != 0 {
                InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_SETFOCUS => {
            reset_caret(hwnd, st());
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            stop_caret(hwnd, st());
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_CARET {
                let s = st();
                s.caret_on = !s.caret_on;
                invalidate_input(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let p = point_from_lparam(lparam);
            let s = st();
            if p.y < TITLE_H {
                if p.x <= 45 {
                    hide_current();
                    return LRESULT(0);
                }
                s.dragging = true;
                let mut gp = POINT::default();
                let _ = GetCursorPos(&mut gp);
                let mut rc = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rc);
                s.drag_off = POINT { x: gp.x - rc.left, y: gp.y - rc.top };
                SetCapture(hwnd);
                return LRESULT(0);
            }
            if p.y < TITLE_H + HEADER_H && p.x > WIN_W - 60 {
                s.input_visible = !s.input_visible;
                if s.input_visible { s.input_buffer.clear(); }
                InvalidateRect(hwnd, None, false);
                reset_caret(hwnd, s);
                return LRESULT(0);
            }
            if s.input_visible && p.y >= TITLE_H + HEADER_H && p.y < TITLE_H + HEADER_H + INPUT_H {
                SetFocus(hwnd);
                reset_caret(hwnd, s);
                return LRESULT(0);
            }
            let top = list_top(s) - s.scroll_y;
            let tasks = s.store.lock().unwrap().all().to_vec();
            let mut idx = 0i32;
            for task in &tasks {
                let y0 = top + idx * (CARD_H + CARD_GAP);
                let y1 = y0 + CARD_H;
                if p.y >= y0 && p.y <= y1 {
                    if p.x >= MARGIN_X + 15 && p.x <= MARGIN_X + 15 + 22 {
                        let id = task.id.clone();
                        s.store.lock().unwrap().toggle(&id);
                        InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                    if p.x >= WIN_W - MARGIN_X - 24 - 12 && p.x <= WIN_W - MARGIN_X {
                        let id = task.id.clone();
                        s.store.lock().unwrap().delete(&id);
                        InvalidateRect(hwnd, None, false);
                        return LRESULT(0);
                    }
                }
                idx += 1;
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let s = st();
            if s.dragging {
                let mut gp = POINT::default();
                let _ = GetCursorPos(&mut gp);
                let nx = gp.x - s.drag_off.x;
                let ny = gp.y - s.drag_off.y;
                SetWindowPos(hwnd, None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                let s = st();
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let _ = ScreenToClient(hwnd, &mut p);
                let icon = if p.y < TITLE_H {
                    if p.x <= 45 { IDC_HAND } else { IDC_ARROW }
                } else if p.y < TITLE_H + HEADER_H {
                    if p.x > WIN_W - 60 { IDC_HAND } else { IDC_ARROW }
                } else if s.input_visible && p.y < TITLE_H + HEADER_H + INPUT_H {
                    IDC_IBEAM
                } else if over_card_action(s, p.x, p.y) {
                    IDC_HAND
                } else {
                    IDC_ARROW
                };
                SetCursor(LoadCursorW(None, icon).unwrap_or_default());
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONUP => {
            st().dragging = false;
            ReleaseCapture();
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let s = st();
            let delta = ((wparam.0 >> 16) as i16) as i32;
            let step = -delta / 120 * (CARD_H + CARD_GAP);
            let maxs = scroll_max(s.store.lock().unwrap().all().len(), list_top(s));
            s.scroll_y = (s.scroll_y + step).clamp(0, maxs);
            InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_CHAR => {
            let s = st();
            if s.input_visible {
                let c = wparam.0 as u32;
                match c {
                    13 => {
                        let title = s.input_buffer.trim().to_string();
                        if !title.is_empty() {
                            s.store.lock().unwrap().add(&title);
                        }
                        s.input_buffer.clear();
                        s.input_visible = false;
                        InvalidateRect(hwnd, None, false);
                        reset_caret(hwnd, s);
                    }
                    8 => {
                        s.input_buffer.pop();
                        invalidate_input(hwnd);
                        reset_caret(hwnd, s);
                    }
                    27 => {
                        s.input_buffer.clear();
                        s.input_visible = false;
                        InvalidateRect(hwnd, None, false);
                        reset_caret(hwnd, s);
                    }
                    _ => {
                        if let Some(ch) = char::from_u32(c) {
                            if !ch.is_control() {
                                s.input_buffer.push(ch);
                                invalidate_input(hwnd);
                                reset_caret(hwnd, s);
                            }
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 打字时只需重绘输入框那一条区域，任务列表保持原样，避免整窗闪烁。
unsafe fn invalidate_input(hwnd: HWND) {
    let rc = RECT {
        left: 0,
        top: TITLE_H + HEADER_H,
        right: WIN_W,
        bottom: TITLE_H + HEADER_H + INPUT_H,
    };
    InvalidateRect(hwnd, Some(&rc), false);
}

/// 圆角抗锯齿：内部像素 alpha=255；四个角落按圆弧覆盖率渐变写 alpha（RGB 同步预乘）。
unsafe fn round_corner_alpha(px: *mut u32, w: i32, h: i32, r: i32) {
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

unsafe fn paint(hwnd: HWND) {
    let s = st();
    let mut ps = PAINTSTRUCT::default();
    let _ = BeginPaint(hwnd, &mut ps);

    // 画入 32 位 DIB，再用 UpdateLayeredWindow 逐像素上屏，
    // 圆角由 alpha 渐变实现抗锯齿（窗口区域裁剪是二值的，做不到平滑边缘）
    let screen = GetDC(None);
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = WIN_W;
    bmi.bmiHeader.biHeight = -WIN_H; // 自上而下
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0 as u32;

    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let bmp = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
    let mem = CreateCompatibleDC(screen);
    let old_bmp = SelectObject(mem, bmp);

    let full = RECT { left: 0, top: 0, right: WIN_W, bottom: WIN_H };
    let bg = CreateSolidBrush(rgb(C_WHITE.0, C_WHITE.1, C_WHITE.2));
    FillRect(mem, &full, bg);
    DeleteObject(bg);

    // 标题栏
    let title_rc = RECT { left: 0, top: 0, right: WIN_W, bottom: TITLE_H };
    gdi::draw_text(mem, "待办事项", title_rc, rgb(C_MUTED.0, C_MUTED.1, C_MUTED.2),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE, s.font_header);
    let close_rc = RECT { left: 0, top: 0, right: 45, bottom: TITLE_H };
    gdi::draw_text(mem, "×", close_rc, rgb(C_MUTED.0, C_MUTED.1, C_MUTED.2),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE, s.font_btn);

    // header
    let hdr_y = TITLE_H;
    let hdr_title = RECT { left: 20, top: hdr_y, right: 200, bottom: hdr_y + HEADER_H };
    gdi::draw_text(mem, "我的待办", hdr_title, rgb(C_TEXT.0, C_TEXT.1, C_TEXT.2),
        DT_LEFT | DT_VCENTER | DT_SINGLELINE, s.font_title);
    let add_rc = RECT { left: WIN_W - 20 - 36, top: hdr_y + 10, right: WIN_W - 20, bottom: hdr_y + 10 + 36 };
    // 直接贴 ➕ emoji 彩色位图
    let pad = 6;
    let img_rc = RECT { left: add_rc.left + pad, top: add_rc.top + pad, right: add_rc.right - pad, bottom: add_rc.bottom - pad };
    gdi::blit_bgra(mem, img_rc, crate::emoji::PLUS_EMOJI, crate::emoji::PLUS_EMOJI_W, crate::emoji::PLUS_EMOJI_H);

    // 输入框
    if s.input_visible {
        let in_y = TITLE_H + HEADER_H;
        let in_rc = RECT { left: 20, top: in_y + 5, right: WIN_W - 20, bottom: in_y + INPUT_H - 5 };
        gdi::round_rect_aa(bits as *mut u32, WIN_W, full, in_rc, 10.0,
            rgb(C_CARD.0, C_CARD.1, C_CARD.2), rgb(C_PURPLE.0, C_PURPLE.1, C_PURPLE.2), 1.0);
        let text = if s.input_buffer.is_empty() { "输入新任务..." } else { &s.input_buffer };
        let color = if s.input_buffer.is_empty() { rgb(150, 150, 150) } else { rgb(C_TEXT.0, C_TEXT.1, C_TEXT.2) };
        let txt_rc = RECT { left: 20 + 12, top: in_y + 5, right: WIN_W - 20 - 12, bottom: in_y + INPUT_H - 5 };
        gdi::draw_text(mem, text, txt_rc, color, DT_LEFT | DT_VCENTER | DT_SINGLELINE, s.font_item);

        // 自绘插入光标（1px 竖线，闪烁节奏与系统一致）
        if s.caret_on && caret_active(hwnd, s) {
            let mut sz = SIZE::default();
            let old_font = SelectObject(mem, s.font_item);
            let wide: Vec<u16> = s.input_buffer.encode_utf16().collect();
            if !wide.is_empty() {
                GetTextExtentPoint32W(mem, &wide, &mut sz);
            }
            SelectObject(mem, old_font);
            let ch = if sz.cy > 0 { sz.cy } else { 20 };
            let left = 20 + 12;
            let x = (left + sz.cx).clamp(left, WIN_W - 20 - 12 - 1);
            let cy = TITLE_H + HEADER_H;
            let caret_rc = RECT { left: x, top: cy + (INPUT_H - ch) / 2, right: x + 1, bottom: cy + (INPUT_H - ch) / 2 + ch };
            let brush = CreateSolidBrush(rgb(C_TEXT.0, C_TEXT.1, C_TEXT.2));
            FillRect(mem, &caret_rc, brush);
            DeleteObject(brush);
        }
    }

    // 任务列表（裁剪到列表区，支持滚轮滚动）
    let top = list_top(s);
    let tasks = s.store.lock().unwrap().all().to_vec();
    let maxs = scroll_max(tasks.len(), top);
    if s.scroll_y > maxs { s.scroll_y = maxs; }
    let saved = SaveDC(mem);
    IntersectClipRect(mem, 0, top, WIN_W, WIN_H);
    let clip = RECT { left: 0, top, right: WIN_W, bottom: WIN_H };
    let mut y = top - s.scroll_y;
    for task in &tasks {
        draw_card(mem, bits as *mut u32, clip, s, task, y);
        y += CARD_H + CARD_GAP;
    }
    RestoreDC(mem, saved);

    round_corner_alpha(bits as *mut u32, WIN_W, WIN_H, CORNER_R);

    let mut rc = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rc);
    let pt_dst = POINT { x: rc.left, y: rc.top };
    let pt_src = POINT { x: 0, y: 0 };
    let sz = SIZE { cx: WIN_W, cy: WIN_H };
    let blend = BLENDFUNCTION {
        BlendOp: 0,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: 1,
    };
    let _ = UpdateLayeredWindow(
        hwnd,
        None,
        Some(&pt_dst),
        Some(&sz),
        mem,
        Some(&pt_src),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );

    SelectObject(mem, old_bmp);
    DeleteObject(bmp);
    DeleteDC(mem);
    ReleaseDC(None, screen);

    EndPaint(hwnd, &ps);
}

unsafe fn draw_card(
    hdc: HDC,
    px: *mut u32,
    clip: RECT,
    s: &TaskWinState,
    task: &crate::store::Task,
    y: i32,
) {
    let bg = if task.completed { C_CARD_DONE } else { C_CARD };
    let card = RECT { left: MARGIN_X, top: y, right: WIN_W - MARGIN_X, bottom: y + CARD_H };
    gdi::round_rect_aa(px, WIN_W, clip, card, 12.0,
        rgb(bg.0, bg.1, bg.2), rgb(C_BORDER.0, C_BORDER.1, C_BORDER.2), 1.0);

    let cx = MARGIN_X + 15 + 11;
    let cy = y + CARD_H / 2;
    let chk = RECT { left: cx - 9, top: cy - 9, right: cx + 9, bottom: cy + 9 };
    if task.completed {
        gdi::fill_ellipse_aa(px, WIN_W, clip, chk, rgb(C_PURPLE.0, C_PURPLE.1, C_PURPLE.2));
    } else {
        gdi::stroke_ellipse_aa(px, WIN_W, clip, chk, 2.0, rgb(C_CHK_BORDER.0, C_CHK_BORDER.1, C_CHK_BORDER.2));
    }

    let text_color = if task.completed { C_MUTED } else { C_TEXT };
    let font = if task.completed { s.font_item_done } else { s.font_item };
    let txt = RECT { left: MARGIN_X + 15 + 34, top: y, right: WIN_W - MARGIN_X - 50, bottom: y + CARD_H };
    gdi::draw_text(hdc, &task.title, txt, rgb(text_color.0, text_color.1, text_color.2),
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS, font);

    let del = RECT { left: WIN_W - MARGIN_X - 24 - 12, top: y, right: WIN_W - MARGIN_X, bottom: y + CARD_H };
    gdi::draw_text(hdc, "×", del, rgb(C_DEL.0, C_DEL.1, C_DEL.2),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE, s.font_btn);
}

fn point_from_lparam(lp: LPARAM) -> POINT {
    let v = lp.0;
    POINT {
        x: (v & 0xFFFF) as i16 as i32,
        y: ((v >> 16) & 0xFFFF) as i16 as i32,
    }
}
