//! 任务窗口 —— 纯 Win32 自绘（无 Edit/ListBox 控件，内存占用最低）。

use crate::gdi::{self, rgb};
use crate::store::TaskStore;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
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
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
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

        let rgn = CreateRoundRectRgn(0, 0, WIN_W + 1, WIN_H + 1, 16, 16);
        SetWindowRgn(hwnd, rgn, TRUE);

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
        }));
        TASKWIN = st;
    }
}

pub fn show_current() {
    unsafe {
        if !TASKWIN.is_null() {
            let st = &*TASKWIN;
            ShowWindow(st.hwnd, SW_SHOW);
            SetForegroundWindow(st.hwnd);
            SetFocus(st.hwnd);
        }
    }
}

pub fn hide_current() {
    unsafe {
        if !TASKWIN.is_null() {
            ShowWindow((*TASKWIN).hwnd, SW_HIDE);
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

unsafe extern "system" fn task_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
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
                InvalidateRect(hwnd, None, TRUE);
                return LRESULT(0);
            }
            if s.input_visible && p.y >= TITLE_H + HEADER_H && p.y < TITLE_H + HEADER_H + INPUT_H {
                SetFocus(hwnd);
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
                        InvalidateRect(hwnd, None, TRUE);
                        return LRESULT(0);
                    }
                    if p.x >= WIN_W - MARGIN_X - 24 - 12 && p.x <= WIN_W - MARGIN_X {
                        let id = task.id.clone();
                        s.store.lock().unwrap().delete(&id);
                        InvalidateRect(hwnd, None, TRUE);
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
            InvalidateRect(hwnd, None, TRUE);
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
                        InvalidateRect(hwnd, None, TRUE);
                    }
                    8 => {
                        s.input_buffer.pop();
                        InvalidateRect(hwnd, None, TRUE);
                    }
                    27 => {
                        s.input_buffer.clear();
                        s.input_visible = false;
                        InvalidateRect(hwnd, None, TRUE);
                    }
                    _ => {
                        if let Some(ch) = char::from_u32(c) {
                            if !ch.is_control() {
                                s.input_buffer.push(ch);
                                InvalidateRect(hwnd, None, TRUE);
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

unsafe fn paint(hwnd: HWND) {
    let s = st();
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let full = RECT { left: 0, top: 0, right: WIN_W, bottom: WIN_H };
    let bg = CreateSolidBrush(rgb(C_WHITE.0, C_WHITE.1, C_WHITE.2));
    FillRect(hdc, &full, bg);
    DeleteObject(bg);

    // 标题栏
    let title_rc = RECT { left: 0, top: 0, right: WIN_W, bottom: TITLE_H };
    gdi::draw_text(hdc, "待办事项", title_rc, rgb(C_MUTED.0, C_MUTED.1, C_MUTED.2),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE, s.font_header);
    let close_rc = RECT { left: 0, top: 0, right: 45, bottom: TITLE_H };
    gdi::draw_text(hdc, "×", close_rc, rgb(C_MUTED.0, C_MUTED.1, C_MUTED.2),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE, s.font_btn);

    // header
    let hdr_y = TITLE_H;
    let hdr_title = RECT { left: 20, top: hdr_y, right: 200, bottom: hdr_y + HEADER_H };
    gdi::draw_text(hdc, "我的待办", hdr_title, rgb(C_TEXT.0, C_TEXT.1, C_TEXT.2),
        DT_LEFT | DT_VCENTER | DT_SINGLELINE, s.font_title);
    let add_rc = RECT { left: WIN_W - 20 - 36, top: hdr_y + 10, right: WIN_W - 20, bottom: hdr_y + 10 + 36 };
    // 直接贴 ➕ emoji 彩色位图
    let pad = 6;
    let img_rc = RECT { left: add_rc.left + pad, top: add_rc.top + pad, right: add_rc.right - pad, bottom: add_rc.bottom - pad };
    gdi::blit_bgra(hdc, img_rc, crate::emoji::PLUS_EMOJI, crate::emoji::PLUS_EMOJI_W, crate::emoji::PLUS_EMOJI_H);

    // 输入框
    if s.input_visible {
        let in_y = TITLE_H + HEADER_H;
        let in_rc = RECT { left: 20, top: in_y + 5, right: WIN_W - 20, bottom: in_y + INPUT_H - 5 };
        gdi::round_rect(hdc, in_rc, 10, rgb(C_CARD.0, C_CARD.1, C_CARD.2), rgb(C_PURPLE.0, C_PURPLE.1, C_PURPLE.2), 1);
        let text = if s.input_buffer.is_empty() { "输入新任务..." } else { &s.input_buffer };
        let color = if s.input_buffer.is_empty() { rgb(150, 150, 150) } else { rgb(C_TEXT.0, C_TEXT.1, C_TEXT.2) };
        let txt_rc = RECT { left: 20 + 12, top: in_y + 5, right: WIN_W - 20 - 12, bottom: in_y + INPUT_H - 5 };
        gdi::draw_text(hdc, text, txt_rc, color, DT_LEFT | DT_VCENTER | DT_SINGLELINE, s.font_item);
    }

    // 任务列表（裁剪到列表区，支持滚轮滚动）
    let top = list_top(s);
    let tasks = s.store.lock().unwrap().all().to_vec();
    let maxs = scroll_max(tasks.len(), top);
    if s.scroll_y > maxs { s.scroll_y = maxs; }
    let saved = SaveDC(hdc);
    IntersectClipRect(hdc, 0, top, WIN_W, WIN_H);
    let mut y = top - s.scroll_y;
    for task in &tasks {
        draw_card(hdc, s, task, y);
        y += CARD_H + CARD_GAP;
    }
    RestoreDC(hdc, saved);

    EndPaint(hwnd, &ps);
}

unsafe fn draw_card(hdc: HDC, s: &TaskWinState, task: &crate::store::Task, y: i32) {
    let bg = if task.completed { C_CARD_DONE } else { C_CARD };
    let card = RECT { left: MARGIN_X, top: y, right: WIN_W - MARGIN_X, bottom: y + CARD_H };
    gdi::round_rect(hdc, card, 12, rgb(bg.0, bg.1, bg.2), rgb(C_BORDER.0, C_BORDER.1, C_BORDER.2), 1);

    let cx = MARGIN_X + 15 + 11;
    let cy = y + CARD_H / 2;
    let chk = RECT { left: cx - 9, top: cy - 9, right: cx + 9, bottom: cy + 9 };
    if task.completed {
        gdi::fill_ellipse(hdc, chk, rgb(C_PURPLE.0, C_PURPLE.1, C_PURPLE.2));
    } else {
        let pen = CreatePen(PS_SOLID, 2, rgb(C_CHK_BORDER.0, C_CHK_BORDER.1, C_CHK_BORDER.2));
        let op = SelectObject(hdc, pen);
        let ob = SelectObject(hdc, GetStockObject(NULL_BRUSH));
        Ellipse(hdc, chk.left, chk.top, chk.right, chk.bottom);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(pen);
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
